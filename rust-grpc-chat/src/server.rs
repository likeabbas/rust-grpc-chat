pub mod chat {
    tonic::include_proto!("chat");
}

use chat::chat_server::{Chat, ChatServer};
use chat::{LoginRequest, LoginResponse, LogoutRequest, LogoutResponse, StreamRequest, StreamResponse, stream_response, stream_request, SenderMessage, ReceiverMessage, SendChatRequest, ServerResponse, UserLoginRequest, server_response};

use std::pin::Pin;
use std::sync::{Arc};
use std::time::{Instant, SystemTime};
use std::str;
use async_trait::async_trait;
use std::collections::HashMap;
use futures::{StreamExt};
use tokio::sync::{mpsc, RwLock};
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
use ringbuf::{RingBuffer, Producer, Consumer};
use prost_types::Timestamp;
use chrono::{DateTime, Duration, Utc};
use std::borrow::Borrow;
use crate::chat::stream_response::Event;
use arraydeque::ArrayDeque;
use async_stream::try_stream;


// use prost_types::field_descriptor_proto::Type::Message;

// TODO
// Implement timeouts for streams

#[derive(Debug, Clone)]
pub struct TokenData {
    username: String,
    expiration: DateTime<Utc>,
}


#[derive(Debug, Clone)]
pub struct UserData {
    token: String,
    expiration: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ChatService {
    username_to_user_password: Arc<DashMap<String,String>>,
    token_to_data: Arc<DashMap<String, TokenData>>,
    token_to_username: Arc<DashMap<String,String>>,
    arr: Arc<RwLock<ArrayDeque<[ReceiverMessage; 10]> >>,
    server_password: &'static str,
    tx: Sender<Result<StreamResponse,Status>>,
    // New Data
    username_to_data: Arc<DashMap<String, RwLock<UserData>>>,
    last_messages: RwLock<ArrayDeque<[ServerResponse]>>,
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
        let name: &String = &request.get_ref().username;
        let received_server_password: &String = &request.get_ref().server_password;

        if received_server_password.as_str() != self.server_password {
            println!("bad pass");
            return Err(Status::new(Code::InvalidArgument, ("Incorrect Password")));
        } else if self.token_to_username.contains_key(name.as_str()) {
            println!("bad name");
            return Err(Status::new(Code::InvalidArgument, ("Username in use")));
        }

        let token = &Uuid::new_v4().to_string();
        self.token_to_username.insert(token.to_string(), name.to_string());
        Ok(Response::new(LoginResponse{token: token.to_string(), recent_messages: vec![] }))
    }

    async fn logout(&self, request: Request<LogoutRequest>) -> Result<Response<LogoutResponse>, Status> {
        unimplemented!()
    }

    type StreamStream =
    Pin<Box<dyn Stream<Item = Result<StreamResponse, Status>> + Send + Sync + 'static>>;

    // TODO
    // Refactor this so that requests don't stream to the server
    // Server shouldn't haven't to `.await` on a
    async fn stream(
        &self,
        request: Request<tonic::Streaming<StreamRequest>>
    ) -> Result<Response<Self::StreamStream>, Status> {
        println!("stream");
        // unimplemented!();

        let token: &str;

        let metadata_clone = request.metadata().clone();
        match self.check_auth(&metadata_clone).await {
            Ok(t) => token = t.clone(),
            Err(status) => return Err(status),
        };

        // let stream = request.into_inner();


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
        // TODO
        // right now, the stream request can never know if it got the message successfully because that's spawned inside
        // another thread.
        spawn_stream(request.into_inner(), self.token_to_username.as_ref().get(token).unwrap().key().clone(), self.tx.clone(), self.token_to_username.clone()/*, self.message_producer.clone()*/);
        // let c = self;

        let now = chrono::offset::Utc::now();
        let expiration = now + Duration::hours(3) ;
        let mut rx = self.tx.subscribe();

        let rx = async_stream::try_stream! {
            println!("in try stream");

            while let Ok(stream_response) = rx.recv().await {
                println!("receiving");
                // if expiration < chrono::offset::Utc::now() {
                //     // grab expiration from hashmap to see if it has changed.
                //     // if that expiration is the same, then boot user
                //     // else just update the expiration thing here
                //     break;
                // }

                println!("{:?}", stream_response);
                match stream_response {
                    Ok(message) => {
                        yield message;
                    },

                    _ => {

                    }
                }

            }
        };

        // turn rx into a broadcast receiver stream that filters out shit
        // let rx = BroadcastStream::new(rx)
        //     .filter_map(|item| async move {
        //         // ignore receive errors
        //         item.ok()
        //     });

        let ret: Self::StreamStream = Box::pin(rx);
        Ok(Response::new(ret))
    }

    type UserLoginStream =
        Pin<Box<dyn Stream<Item = Result<ServerResponse, Status>> + Send + Sync + 'static>>;

    async fn user_login(&self, request: Request<UserLoginRequest>) -> Result<Response<Self::UserLoginStream>, Status> {
        let username = request.into_inner().username;

        if self.username_to_data.contains_key(&*username) {
            return Err(Status::new(Code::Unavailable, "name is already taken"))
        }

        let user_data = UserData {
            token: Uuid::new_v4().to_string(),

        };
        todo!()
    }

    async fn user_logout(&self, request: Request<chat_client::chat::UserLogoutRequest>) -> Result<Response<()>, Status> {
        todo!()
    }

    async fn send_chat(&self, request: Request<chat_client::chat::SendChatRequest>) -> Result<Response<()>, Status> {
        todo!()
    }
}



fn spawn_stream(mut req_stream: tonic::Streaming<StreamRequest>, name: String, tx: Sender<Result<StreamResponse, Status>>, token_to_username: Arc<DashMap<String,String>>/*, mut message_producer: Arc<Producer<stream_response::Message>>*/) {

    tokio::spawn(async move {
        let name = &name;
        loop {

            match req_stream.next().await {
                Some(t) => {
                    match t {
                        Ok(r) => {
                            //TODO
                            //check if token close to expiration but still <. if so, update token so he can continue receiving messages

                            match r.event.unwrap() {
                                //https://doc.rust-lang.org/std/convert/trait.From.html
                                stream_request::Event::SendMessage(sender_message) => {
                                    // let message = Message{username: name.to_string(), content: send.content, timestamp: Timestamp {seconds: SystemTime::now()} };
                                    let message = ReceiverMessage{
                                        username: name.to_string(),
                                        content: sender_message.content,
                                        timestamp: Option::from(Timestamp { seconds: chrono::offset::Utc::now().timestamp(), nanos: 0 })
                                    };
                                    // message_producer.push(message.clone());
                                    // TODO
                                    //
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
    let (tx, mut rx): (Sender<Result<StreamResponse,Status>>, Receiver<Result<StreamResponse,Status>>) = broadcast::channel(100);

    let mut service = ChatService {
        username_to_user_password: Arc::new(DashMap::new()),
        token_to_username: Arc::new(DashMap::new()),
        token_to_data: Arc::new(DashMap::new()),
        server_password: "123".as_ref(),
        arr: Arc::new(RwLock::new(ArrayDeque::new())),
        tx,
    };

    // spawn a thread for the rx to receive messages and update the ring buffer
    let arr = service.arr.clone();
    tokio::spawn(async move {
        // let arr = arr;
        while let Ok(stream_response) = rx.recv().await.unwrap() {
            match stream_response.event.unwrap() {
                Event::ClientLogin(_) => {}
                Event::ClientLogout(_) => {}
                Event::ClientMessage(message) => {
                    let mut x = arr.write().await;
                    if x.is_full() {
                        x.pop_front();
                    }
                    x.push_back(message);
                }
                Event::ServerShutdown(_) => {
                    break;
                }
            }
        }


        // Ok(())
    });



    // ChatServer::with_interceptor();
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


