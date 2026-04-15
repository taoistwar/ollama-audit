use std::net::SocketAddr;

pub fn get_bind_addr() -> SocketAddr {
  let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:5000".to_string());
  let addr: SocketAddr = bind
      .parse()
      .unwrap_or_else(|e| panic!("invalid BIND_ADDR {bind:?}: {e}"));
  addr
}