//! Integration: `s0 send` drives `s0 recv` over a local PTY at 200 Hz; nothing may be lost.
#[test]
fn local_path_zero_loss() {
    let dir = std::path::PathBuf::from("/data/scratch/s0/test"); std::fs::create_dir_all(&dir).unwrap();
    let pid = std::process::id();
    let send_log = dir.join(format!("send-{pid}.jsonl"));
    let recv_log = dir.join(format!("recv-local-{pid}.jsonl"));
    let bin = env!("CARGO_BIN_EXE_s0");
    let st = std::process::Command::new(bin)
        .args(["send", "--log", send_log.to_str().unwrap(), "--events", "300", "--rate", "200", "--settle-ms", "2000", "--",
               bin, "recv", "--log", recv_log.to_str().unwrap(), "--quit-after", "300"])
        .status().unwrap();
    assert!(st.success());
    let send: Vec<serde_json::Value> = std::fs::read_to_string(&send_log).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let acked = send.iter().filter(|v| v.get("ack_ns").map_or(false, |a| !a.is_null())).count();
    assert_eq!(acked, 300, "every event acked");
    let recv_n = std::fs::read_to_string(&recv_log).unwrap().lines().filter(|l| l.contains("\"seq\"")).count();
    assert_eq!(recv_n, 300);
    let done = send.last().unwrap();
    assert_eq!(done["done"], true);
    assert_eq!(done["acked"], 300);
    assert_eq!(done["saw_done"], true);
    assert_eq!(done["events"], 300);
}
