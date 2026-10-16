#![no_main]
//! The newline-delimited framer under stdio: arbitrary bytes, a small line
//! cap, read to the end. Every outcome is a frame, a recoverable bad frame,
//! or a fatal error; never a panic or a hang.

use libfuzzer_sys::fuzz_target;
use turbomcp_core::codec::SerdeJsonCodec;
use turbomcp_service::Transport;
use turbomcp_service::io::LineTransport;

/// Whether the framer gave up on the stream rather than one frame.
fn fatal<T: Transport>(_: &T, error: T::Error) -> bool {
    T::invalid_frame(error).is_err()
}

fuzz_target!(|data: Vec<u8>| {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let reader = tokio::io::BufReader::new(std::io::Cursor::new(data));
        let mut transport =
            LineTransport::new(reader, tokio::io::sink(), SerdeJsonCodec).with_max_line_bytes(256);
        for _ in 0..4096 {
            match transport.recv().await {
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(e) => {
                    if fatal(&transport, e) {
                        break;
                    }
                }
            }
        }
    });
});
