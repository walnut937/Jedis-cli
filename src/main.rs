use std::future::Future;
use std::io::{self, Write};
use std::pin::Pin;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

fn main() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run());
}

async fn run() {
    let host = std::env::args().nth(1).unwrap_or("127.0.0.1".to_string());
    let port = std::env::args().nth(2).unwrap_or("6379".to_string());
    let addr = format!("{}:{}", host, port);

    let stream = match TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to connect to {}: {}", addr, e);
            std::process::exit(1);
        }
    };

    println!("Connected to {} — jedis-cli 0.1.0", addr);
    println!("Type 'quit' or 'exit' to leave\n");

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    loop {
        print!("{}> ", addr);
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if input.eq_ignore_ascii_case("quit") || input.eq_ignore_ascii_case("exit") {
            println!("Bye!");
            break;
        }

        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let resp = format_resp(&parts);
        if writer.write_all(resp.as_bytes()).await.is_err() {
            eprintln!("Server disconnected");
            std::process::exit(0);
        }

        if parts[0].eq_ignore_ascii_case("monitor") {
            match read_response(&mut reader).await {
                Ok(r) => println!("{}", r),
                Err(_) => {
                    eprintln!("Server disconnected");
                    std::process::exit(0);
                }
            }
            run_monitor_mode(&mut reader).await;
            std::process::exit(0);
        }

        match read_response(&mut reader).await {
            Ok(response) => print_response(&response),
            Err(e) => {
                eprintln!("\nServer disconnected: {}", e);
                std::process::exit(0);
            }
        }
    }
}

async fn run_monitor_mode(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) {
    println!("Entering monitor mode. Press Ctrl+C to exit.\n");
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                println!("\nServer disconnected");
                std::process::exit(0);
            }
            Ok(_) => {
                let line = line.trim_end_matches("\r\n");
                if line.starts_with('+') {
                    println!("{}", &line[1..]);
                } else {
                    println!("{}", line);
                }
            }
            Err(_) => {
                println!("\nServer disconnected");
                std::process::exit(0);
            }
        }
    }
}

fn format_resp(parts: &[&str]) -> String {
    let mut resp = format!("*{}\r\n", parts.len());
    for part in parts {
        resp.push_str(&format!("${}\r\n{}\r\n", part.len(), part));
    }
    resp
}

fn read_response<'a>(
    reader: &'a mut BufReader<tokio::net::tcp::OwnedReadHalf>,
) -> Pin<Box<dyn Future<Output = Result<String, String>> + 'a>> {
    Box::pin(async move {
        let mut line = String::new();

        match reader.read_line(&mut line).await {
            Ok(0) => {
                println!("\nServer disconnected");
                std::process::exit(0);
            }
            Ok(_) => {}
            Err(e) => {
                println!("\nServer disconnected: {}", e);
                std::process::exit(0);
            }
        }

        let line = line.trim_end_matches("\r\n");

        match line.chars().next() {
            Some('+') => Ok(line[1..].to_string()),
            Some('-') => Ok(format!("ERR {}", &line[1..])),
            Some(':') => Ok(line[1..].to_string()),

            Some('$') => {
                let len: i64 = line[1..]
                    .parse()
                    .map_err(|e: std::num::ParseIntError| e.to_string())?;
                if len == -1 {
                    Ok("nil".to_string())
                } else {
                    let mut bulk = String::new();
                    reader
                        .read_line(&mut bulk)
                        .await
                        .map_err(|e| e.to_string())?;
                    Ok(bulk.trim_end_matches("\r\n").to_string())
                }
            }

            Some('*') => {
                let count: i64 = line[1..]
                    .parse()
                    .map_err(|e: std::num::ParseIntError| e.to_string())?;
                if count == -1 {
                    Ok("nil".to_string())
                } else {
                    let mut items = Vec::new();
                    for _ in 0..count {
                        let item = read_response(reader).await?;
                        items.push(item);
                    }
                    Ok(items
                        .into_iter()
                        .enumerate()
                        .map(|(i, v)| format!("{}) {}", i + 1, v))
                        .collect::<Vec<_>>()
                        .join("\n"))
                }
            }

            _ => Ok(line.to_string()),
        }
    })
}

fn print_response(response: &str) {
    for line in response.lines() {
        if line.is_empty() {
            continue;
        }
        if line.starts_with("ERR") {
            println!("(error) {}", line);
        } else if line == "nil" {
            println!("(nil)");
        } else if line.parse::<i64>().is_ok() {
            println!("(integer) {}", line);
        } else {
            println!("{}", line);
        }
    }
}
