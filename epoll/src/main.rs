use mio::{Events, Interest, Poll, Token, event};
use std::{collections::HashMap, io::{Read, Write}};
use mio::net::{TcpListener, TcpStream};
use std::net::{self, SocketAddr};

const SERVER: Token = Token(0);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut poll = Poll::new()?;

    let addr: SocketAddr = "127.0.0.1:8080".parse()?;

    let mut listener = TcpListener::bind(addr)?;

    poll.registry().register(
        &mut listener,
        SERVER,
        Interest::READABLE ,
    )?;

    let mut events = Events::with_capacity(1024);

    let mut connections: HashMap<Token, mio::net::TcpStream> = HashMap::new();

    let mut next_token = 1usize;  // SERVER=0, clients start at 1


    loop {
        poll.poll(&mut events, None)?;

        for event in &events {
            match event.token() {
                SERVER => {
                    let (mut stream, addr) = listener.accept()?;
                    println!("new connection from {}",addr);
                    let token = Token(next_token);
                    next_token += 1;

                    poll.registry().register(&mut stream, token, Interest::READABLE)?;
                    connections.insert(token, stream);
                }
                token => {
                    let stream = connections.get_mut(&token).unwrap();   
                    let mut buf = [0u8; 512];
                    let n = stream.read(&mut buf).unwrap();
                    let command = String::from_utf8_lossy(&buf[..n]);
                    println!("message :{}",command);
                    stream.write(b"Hello\n")?;
                    poll.registry().deregister(stream)?;
                    connections.remove(&token);
                }
            }
        }
    }
}
