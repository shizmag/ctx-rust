use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use ctx_llm::{LlmClient, Provider};

fn start_mock_server(response_body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = Vec::new();
            let mut temp_buf = [0; 1024];
            let mut content_length = None;
            
            while let Ok(n) = stream.read(&mut temp_buf) {
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&temp_buf[..n]);
                
                if content_length.is_none() {
                    if let Some(pos) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers_str = String::from_utf8_lossy(&request[..pos]);
                        for line in headers_str.lines() {
                            if line.to_lowercase().starts_with("content-length:") {
                                if let Some((_, val)) = line.split_once(':') {
                                    if let Ok(len) = val.trim().parse::<usize>() {
                                        content_length = Some(len);
                                    }
                                }
                            }
                        }
                        if content_length.is_none() {
                            break;
                        }
                    }
                }
                
                if let Some(len) = content_length {
                    if let Some(pos) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        let total_expected = pos + 4 + len;
                        if request.len() >= total_expected {
                            break;
                        }
                    }
                }
            }
            
            let http_response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            
            let _ = stream.write_all(http_response.as_bytes());
            let _ = stream.flush();
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    });

    format!("http://127.0.0.1:{}", port)
}

#[test]
fn test_openai_provider() {
    let mock_response = r#"{
        "choices": [
            {
                "message": {
                    "role": "assistant",
                    "content": "Hello from mock OpenAI!"
                }
            }
        ]
    }"#;

    let base_url = start_mock_server(mock_response);
    let client = LlmClient::new(Provider::OpenAi, "mock_key".to_string(), "gpt-4".to_string())
        .with_base_url(base_url);

    let res = client.generate_response("hello").unwrap();
    assert_eq!(res, "Hello from mock OpenAI!");
}

#[test]
fn test_anthropic_provider() {
    let mock_response = r#"{
        "content": [
            {
                "type": "text",
                "text": "Hello from mock Anthropic!"
            }
        ]
    }"#;

    let base_url = start_mock_server(mock_response);
    let client = LlmClient::new(Provider::Anthropic, "mock_key".to_string(), "claude-3".to_string())
        .with_base_url(base_url);

    let res = client.generate_response("hello").unwrap();
    assert_eq!(res, "Hello from mock Anthropic!");
}

#[test]
fn test_gemini_provider() {
    let mock_response = r#"{
        "candidates": [
            {
                "content": {
                    "parts": [
                        {
                            "text": "Hello from mock Gemini!"
                        }
                    ]
                }
            }
        ]
    }"#;

    let base_url = start_mock_server(mock_response);
    let client = LlmClient::new(Provider::Gemini, "mock_key".to_string(), "gemini-1.5-flash".to_string())
        .with_base_url(base_url);

    let res = client.generate_response("hello").unwrap();
    assert_eq!(res, "Hello from mock Gemini!");
}
