#![recursion_limit="256"]

pub mod chat {
    tonic::include_proto!("chat");
}

use chat::chat_client::ChatClient;
use chat::{LoginRequest, LoginResponse, LogoutRequest, LogoutResponse, StreamRequest, StreamResponse, stream_response, stream_request, SenderMessage};

use std::error::Error;
use std::time::Duration;

use futures::stream;
use rand::rngs::ThreadRng;
use rand::Rng;
use tokio::time;
use tonic::transport::Channel;
use tokio::io::{stdin, AsyncRead};
use tonic::{Request, Response, Status, Code};
use tokio_util::codec::{FramedRead, LinesCodec};
use tokio_stream::StreamExt;
use async_stream::stream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Enter your name: ");
    let mut name = String::new();
    std::io::stdin().read_line(&mut name).expect("Failed to read line");

    let mut client = ChatClient::connect("http://[::1]:10000").await?;
    let response = client
        .login(Request::new(LoginRequest {username: name, user_password: "".to_string(), server_password: "123".to_string()}))
        .await?;
    // println!("{}", &response.into_inner().token);
    // let resp = response.get_ref();
    // let token = resp.token.as_str();
    run_route_chat(&mut client, response.get_ref().token.clone()).await;
    Ok(())
}

async fn run_route_chat(client: &mut ChatClient<Channel>, token: String) -> Result<(), Box<dyn Error>> {
    let start = time::Instant::now();
    println!("run_route_chat");
    let t = token.clone();
    let mut reader = FramedRead::new(stdin(), LinesCodec::new());
    let default = Option::from(stream_request::Event::EndChat(stream_request::End {token: t.clone()}));

    // Leaving this uncommented makes the first chat read in not work
    // let r = &reader.next()
    //     .await
    //     .transpose()
    //     .map_or_else(|e| default.clone(), |o| o.map_or(default.clone(), |message|  Option::from(stream_request::Event::SendMessage( SenderMessage{content: message}))));

    // let outbound = async_stream::stream! {
    //     let mut interval = time::interval(Duration::from_secs(1));
    //
    //     while let time = interval.tick().await {
    //         let elapsed = time.duration_since(start);
    //         let note = StreamRequest {
    //             message: format!("at {:?}", elapsed),
    //         };
    //
    //         yield note;
    //     }
    // };


    let outbound = stream! {
        let t = t;
        let default = Option::from(stream_request::Event::EndChat(stream_request::End {token: t}));
        loop {
            let r = &reader.next()
                .await
                .transpose()
                .map_or_else(|e| default.clone(), |o| o.map_or(default.clone(), |message|  Option::from(stream_request::Event::SendMessage(SenderMessage {content: message}))));

            match r.as_ref().unwrap() {
                stream_request::Event::SendMessage(send) => {
                    let req = StreamRequest {
                        event: r.to_owned()
                    };
                    yield req;
                }
                stream_request::Event::EndChat(end) => {
                    let req = StreamRequest {
                        event: r.to_owned()
                    };
                    yield req;
                    break;
                }
            }
        }
    };


    let mut request = Request::new(outbound);
    request.metadata_mut().append("authorization", token.parse().unwrap());
    let response = client.stream(request).await?;
    let mut inbound = response.into_inner();

    while let Some(note) = inbound.message().await? {
        println!("Message = {:?}", note);
    }

    Ok(())
}


