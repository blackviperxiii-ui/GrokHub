//! One-shot ACP ping using the same client as the cabin.
use grokhub_acp::{connect, wait_event, AcpEvent, SessionMode, SpawnOpts};
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    let cwd = PathBuf::from("/home/viper/GrokHub-Work");
    let grok = grokhub_acp::find_grok().expect("grok");
    let opts = SpawnOpts {
        program: grok,
        args: grokhub_acp::agent_args(false),
        cwd,
        api_key: grokhub_acp::grok_cli_key(),
        xai_api_key: None,
        always_approve: false,
        auto: true,
        session_mode: SessionMode::Chat,
        extra_env: vec![],
        handshake_timeout: None,
        resume: None,
    };
    eprintln!("connect…");
    let h = connect(opts).expect("connect");
    eprintln!("session {}", h.session_id);
    h.prompt("reply with the single word pong").expect("prompt");
    let mut n = 0;
    for _ in 0..80 {
        match wait_event(&h.events, Duration::from_secs(2)) {
            Ok(AcpEvent::Ready { .. }) => eprintln!("ready"),
            Ok(AcpEvent::Thought(t)) => {
                n += 1;
                if n < 8 {
                    eprint!("{t}");
                }
            }
            Ok(AcpEvent::Text(t)) => {
                eprintln!("\nTEXT {t}");
            }
            Ok(AcpEvent::Done { stop_reason }) => {
                eprintln!("\nDONE {stop_reason}");
                return;
            }
            Ok(AcpEvent::Err(e)) => {
                eprintln!("\nERR {e}");
                std::process::exit(2);
            }
            Ok(other) => eprintln!("ev {other:?}"),
            Err(e) => {
                eprintln!("\nWAIT {e} n={n}");
                if n == 0 {
                    std::process::exit(3);
                }
            }
        }
    }
    eprintln!("timeout n={n}");
    std::process::exit(4);
}
