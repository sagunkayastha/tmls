//! Command line: the flags of the shared contract, nothing else.

use std::path::PathBuf;

use s2_wgpu::cli::{Args, Load, Present, parse};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn defaults() {
    let shell = Some("/bin/zsh".to_string());
    assert_eq!(
        parse(args(&[]), shell),
        Ok(Args {
            bench: None,
            photon: false,
            cmd: vec!["/bin/zsh".to_string()],
            present: Present::Fifo,
            load_label: Load::Quiet,
        })
    );
    // No $SHELL (or an empty one): a shell that always exists.
    assert_eq!(parse(args(&[]), None).unwrap().cmd, ["/bin/sh"]);
    assert_eq!(
        parse(args(&[]), Some(String::new())).unwrap().cmd,
        ["/bin/sh"]
    );
}

#[test]
fn every_contract_flag() {
    let parsed = parse(
        args(&[
            "--bench",
            "/data/scratch/s2/x",
            "--photon",
            "--present",
            "mailbox",
            "--load-label",
            "flood",
            "--cmd",
            "s0",
            "flood",
            "--bytes-per-sec",
            "500000",
        ]),
        Some("/bin/zsh".to_string()),
    )
    .unwrap();
    assert_eq!(
        parsed,
        Args {
            bench: Some(PathBuf::from("/data/scratch/s2/x")),
            photon: true,
            cmd: args(&["s0", "flood", "--bytes-per-sec", "500000"]),
            present: Present::Mailbox,
            load_label: Load::Flood,
        }
    );

    for (word, mode) in [
        ("fifo", Present::Fifo),
        ("mailbox", Present::Mailbox),
        ("immediate", Present::Immediate),
    ] {
        assert_eq!(
            parse(args(&["--present", word]), None).unwrap().present,
            mode
        );
        assert_eq!(mode.as_str(), word, "the word written to the meta line");
    }
    for (word, load) in [("quiet", Load::Quiet), ("flood", Load::Flood)] {
        assert_eq!(
            parse(args(&["--load-label", word]), None)
                .unwrap()
                .load_label,
            load
        );
        assert_eq!(load.as_str(), word);
    }
}

#[test]
fn cmd_takes_the_rest_of_the_line() {
    // Flags after `--cmd <program>` belong to the program, also ones this app knows.
    let parsed = parse(
        args(&[
            "--photon", "--cmd", "sh", "-c", "echo hi", "--photon", "--bench", "x",
        ]),
        None,
    )
    .unwrap();
    assert_eq!(
        parsed.cmd,
        args(&["sh", "-c", "echo hi", "--photon", "--bench", "x"])
    );
    assert!(parsed.photon);
    assert_eq!(parsed.bench, None);
}

#[test]
fn errors_name_the_problem() {
    let err = |list: &[&str]| parse(args(list), None).unwrap_err();
    assert!(err(&["--vsync"]).contains("--vsync"));
    assert!(err(&["stray"]).contains("stray"));
    assert!(err(&["--bench"]).contains("--bench"));
    assert!(err(&["--present"]).contains("--present"));
    assert!(err(&["--present", "vsync"]).contains("vsync"));
    assert!(err(&["--load-label", "heavy"]).contains("heavy"));
    assert!(err(&["--cmd"]).contains("--cmd"));
    // A flag is not taken as another flag's value.
    assert!(err(&["--bench", "--photon"]).contains("--bench"));
}
