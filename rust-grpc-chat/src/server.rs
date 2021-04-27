pub mod chat {
    tonic::include_proto!("chat");
}

use chat::chat_server::{Chat, ChatServer};
use chat::{LoginRequest, LoginResponse, LogoutRequest, LogoutResponse, StreamRequest, StreamResponse, stream_response, stream_request};

use std::pin::Pin;
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use std::str;
use async_trait::async_trait;
use std::collections::HashMap;
use futures::{StreamExt};
use tokio::sync::mpsc;
use tokio::sync::broadcast;
use tokio_stream::{wrappers::BroadcastStream, Stream};
use tonic::transport::Server;
use tonic::{metadata::MetadataValue, Request, Response, Status, Code};
use uuid::Uuid;
use std::cell::RefCell;
use dashmap::DashMap;
use tonic::metadata::{AsciiMetadataValue, MetadataMap};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio_stream::wrappers::ReceiverStream;

use prost_types::field_descriptor_proto::Type::Message;


#[derive(Debug, Clone)]
pub struct ChatService {
    token_to_username: Arc<DashMap<String,String>>,
    password: &'static str,
    tx: Sender<Result<StreamResponse,Status>>,
}

#[async_trait]
trait Auth {
    async fn check_auth<'a>(&'a self, metadata_map: &'a MetadataMap) -> Result<&str, Status>;
}

#[async_trait]
impl Auth for ChatService {
    async fn check_auth<'a>(&'a self, metadata_map: &'a MetadataMap) -> Result<&str, Status> {
        // let token = MetadataValue::from_str("Bearer some-secret-token").unwrap();
        let opt_token = metadata_map.get("authorization");
        return match opt_token {
            Some(ascii) => {
                match str::from_utf8(ascii.as_ref()) {
                    Ok(token) if self.token_to_username.contains_key(token) => Ok(token),
                    _ => {
                        println!("invalid auth token");
                        Err(Status::unauthenticated("Invalid auth token"))
                    }
                }
            },
            _ =>  {
                println!("not valid auth token");
                Err(Status::unauthenticated("No valid auth token"))
            },
        }
    }
}

#[tonic::async_trait]
impl<'a> Chat for ChatService {

    async fn login(&self, request: Request<LoginRequest>) -> Result<Response<LoginResponse>, Status> {
        let name: &String = &request.get_ref().name;
        let password: &String = &request.get_ref().password;

        if password.as_str() != self.password {
            println!("bad pass");
            return Err(Status::new(Code::InvalidArgument, ("Incorrect Password")));
        } else if self.token_to_username.contains_key(name.as_str()) {
            println!("bad name");
            return Err(Status::new(Code::InvalidArgument, ("Username in use")));
        }

        let token = &Uuid::new_v4().to_string();
        self.token_to_username.insert(token.to_string(), name.to_string());
        Ok(Response::new(LoginResponse{token: token.to_string()}))
    }

    async fn logout(&self, request: Request<LogoutRequest>) -> Result<Response<LogoutResponse>, Status> {
        unimplemented!()
    }

    type StreamStream =
    Pin<Box<dyn Stream<Item = Result<StreamResponse, Status>> + Send + Sync + 'static>>;

    async fn stream(
        &self,
        request: Request<tonic::Streaming<StreamRequest>>
    ) -> Result<Response<Self::StreamStream>, Status> {
        println!("stream");
        // unimplemented!();
        let metadata_clone = request.metadata().clone();
        let token;
        match self.check_auth(&metadata_clone).await {
            Ok(t) => token = t,
            Err(status) => return Err(status),
        };

        println!("token: {}", token);


        // tokio::spawn(async move {
        //     let mut req_stream = request.into_inner();
        //     loop {
        //         match req_stream.next().await {
        //             Some(t) => {
        //                 match t {
        //                     Ok(r) => {
        //                         let message = stream_response::Message{name: name.to_string(), message: r.message};
        //                         tx.send(Ok(StreamResponse {event: Option::from(stream_response::Event::ClientMessage(message))}))
        //                     },
        //                     Err(status) => break
        //                 }
        //             },
        //             None => break
        //         };
        //     }
        // });
        spawn_stream(request.into_inner(), self.token_to_username.as_ref().get(token).unwrap().key().clone(), self.tx.clone(), self.token_to_username.clone());
        let rx = self.tx.subscribe();

        // turn rx into a receiver stream
        let stream = BroadcastStream::new(rx)
            .filter_map(|item| async move {
                // ignore receive errors
                item.ok()
            });

        let ret: Self::StreamStream = Box::pin(stream);
        Ok(Response::new(ret))
    }
}



fn spawn_stream(mut req_stream: tonic::Streaming<StreamRequest>, name: String, tx: Sender<Result<StreamResponse, Status>>, token_to_username: Arc<DashMap<String,String>>) {

    tokio::spawn(async move {
        let name = &name;
        loop {
            match req_stream.next().await {
                Some(t) => {
                    match t {
                        Ok(r) => {

                            match r.event.unwrap() {
                                //https://doc.rust-lang.org/std/convert/trait.From.html
                                stream_request::Event::SendMessage(send) => {
                                    let message = stream_response::Message{name: name.to_string(), message: send.content};
                                    tx.send(Ok(StreamResponse {event: Option::from(stream_response::Event::ClientMessage(message))}));
                                }
                                stream_request::Event::EndChat(end) => {
                                    token_to_username.remove(&*end.token);
                                }
                            }

                        },
                        Err(_status) => break
                    }
                },
                None => break
            };
        }
    });
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:10000".parse().unwrap();
    let (tx, mut _rx1): (Sender<Result<StreamResponse,Status>>, Receiver<Result<StreamResponse,Status>>) = broadcast::channel(32);

    let mut service = ChatService {
        token_to_username: Arc::new(DashMap::new()),
        password: "123".as_ref(),
        tx,
    };


    let svc = ChatServer::new(service);
    Server::builder().add_service(svc).serve(addr).await?;
    Ok(())
    // test.users.insert("abbas".parse().unwrap(), "123".parse().unwrap());
    // println!("{:?}", test.users);
    // let mut rt = tokio::runtime::Runtime::new().unwrap();
    // let cap = async {
    //     println!("hello {}", test.login(Request::new(LoginRequest {password: "123".to_string(), name: "other".to_string()})).await.unwrap().get_ref().token);
    // };
    // rt.block_on(cap);
}
