//! The new-session form, the rename field, the kill confirm and the row menu: keys, clicks
//! and what they ask for. No window, no tmux, no ssh: whatever would run a command, start a
//! terminal or set the clipboard is given a stand-in.

use std::cell::RefCell;

use client_core::tmux::{
    self, NewSession, Start, attach_argv, attach_text, derived_name_argv, kill_argv,
    new_session_argv, rename_argv, running_commands_argv, valid_session_name,
};
use client_core::{SessionInfo, SessionKey, Status, Tool};
use desktop::forms::{
    self, Form, FormResult, Look, MENU, MenuItem, MenuResult, Panel, Request, RowMenu, Stop,
};
use s2_core::{Key, KeyEvent, Mods, Named};

const LOCAL: &str = "here";

fn hosts() -> Vec<String> {
    vec![LOCAL.to_string(), "ph".to_string()]
}

fn presets() -> Vec<(String, Vec<String>)> {
    vec![
        (
            "opus".to_string(),
            vec![
                "claude".to_string(),
                "--model".to_string(),
                "opus".to_string(),
            ],
        ),
        ("codex".to_string(), vec!["codex".to_string()]),
    ]
}

fn key(host: &str, name: &str) -> SessionKey {
    SessionKey {
        host: host.to_string(),
        name: name.to_string(),
    }
}

fn event(key: Key, mods: Mods) -> KeyEvent {
    let text = match key {
        Key::Char(c) if !mods.ctrl && !mods.alt => Some(c.to_string()),
        _ => None,
    };
    KeyEvent {
        key,
        mods,
        text,
        repeat: false,
        pressed: true,
    }
}

fn named(named: Named) -> KeyEvent {
    event(Key::Named(named), Mods::default())
}

fn shift(named: Named) -> KeyEvent {
    let mods = Mods {
        shift: true,
        ..Mods::default()
    };
    event(Key::Named(named), mods)
}

/// Types `text`; every key must leave the form open.
fn type_text(form: &mut Form, text: &str) {
    for c in text.chars() {
        let result = form.key(&event(Key::Char(c), Mods::default()));
        assert_eq!(result, FormResult::Pending, "typing {c:?}");
    }
}

fn tab(form: &mut Form, times: usize) {
    for _ in 0..times {
        assert_eq!(form.key(&named(Named::Tab)), FormResult::Pending);
    }
}

fn new_form(presets: &[(String, Vec<String>)]) -> Form {
    Form::new_session(&hosts(), LOCAL, presets)
}

fn new(form: &Form) -> &forms::NewForm {
    match form {
        Form::New(new) => new,
        other => panic!("not the new-session form: {other:?}"),
    }
}

/// Everything a panel says, line by line.
fn text(panel: &Panel) -> String {
    let mut cells: Vec<_> = panel.cells.iter().collect();
    cells.sort_by_key(|cell| (cell.line, cell.col));
    cells
        .iter()
        .map(|cell| format!("{}:{}", cell.line, cell.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The middle of the cell that says `label`, in the panel's cells.
fn at(panel: &Panel, label: &str) -> (f32, f32) {
    let cell = panel
        .cells
        .iter()
        .find(|cell| cell.text.trim_start_matches(['○', '●', ' ']).trim_end() == label)
        .unwrap_or_else(|| panic!("no cell says {label:?} in\n{}", text(panel)));
    (
        cell.col as f32 + cell.cols as f32 / 2.0,
        cell.line as f32 + 0.5,
    )
}

#[test]
fn tab_order_is_host_folder_name_start_create_cancel() {
    let mut form = new_form(&presets());
    let mut order = vec![form.focus()];
    for _ in 0..6 {
        tab(&mut form, 1);
        order.push(form.focus());
    }
    assert_eq!(
        order,
        [
            Stop::Host,
            Stop::Folder,
            Stop::Name,
            Stop::Start,
            Stop::Ok,
            Stop::Cancel,
            Stop::Host
        ]
    );
    assert_eq!(form.key(&shift(Named::Tab)), FormResult::Pending);
    assert_eq!(form.focus(), Stop::Cancel, "Shift+Tab goes back");
    // The buttons say what they do.
    let said = text(&form.panel());
    assert!(said.contains("Create") && said.contains("Cancel"), "{said}");
}

#[test]
fn the_host_and_the_start_are_chosen_with_the_arrows() {
    let mut form = new_form(&presets());
    assert_eq!(new(&form).host_name(), LOCAL);
    form.key(&named(Named::Right));
    assert_eq!(new(&form).host_name(), "ph");
    form.key(&named(Named::Right));
    assert_eq!(new(&form).host_name(), LOCAL, "the hosts go round");
    // A letter typed on the host changes nothing.
    type_text(&mut form, "x");
    assert_eq!(new(&form).folder.text(), "~");
    assert_eq!(new(&form).name.text(), "home");

    tab(&mut form, 3);
    assert_eq!(form.focus(), Stop::Start);
    form.key(&named(Named::Up));
    assert_eq!(new(&form).start, 1);
    form.key(&named(Named::Down));
    form.key(&named(Named::Down));
    assert_eq!(new(&form).start, 3);
}

#[test]
fn name_follows_the_folder_until_edited() {
    let mut form = new_form(&[]);
    assert_eq!(new(&form).folder.text(), "~");
    assert_eq!(new(&form).name.text(), "home");
    let Form::New(first) = &mut form else {
        unreachable!()
    };
    assert_eq!(first.lookup(), None, "nothing to look up before a change");

    // Tab to the folder: its text is selected, so typing replaces it.
    tab(&mut form, 1);
    type_text(&mut form, "~/proj/app");
    assert_eq!(new(&form).folder.text(), "~/proj/app");
    assert_eq!(
        new(&form).name.text(),
        "app",
        "the folder's own name at once"
    );
    assert!(!new(&form).name_edited);

    // The git-derived name: asked for once per folder, through the injected runner.
    let Form::New(form_new) = &mut form else {
        unreachable!()
    };
    let asked = form_new.lookup().expect("the folder changed");
    assert_eq!(asked, (LOCAL.to_string(), "~/proj/app".to_string()));
    assert_eq!(form_new.lookup(), None, "asked once");
    let ran = RefCell::new(Vec::new());
    let runner = |argv: &[String]| {
        ran.borrow_mut().push(argv.to_vec());
        Ok("the-repo\n".to_string())
    };
    let derived = forms::derived_name(&asked.0, &asked.1, LOCAL, &runner);
    assert_eq!(
        *ran.borrow(),
        [derived_name_argv(LOCAL, "~/proj/app", LOCAL)]
    );
    assert_eq!(derived.as_deref(), Some("the-repo"));
    // An answer for a folder that is no longer the form's is dropped.
    form_new.derived(LOCAL, "~/proj", "stale");
    assert_eq!(form_new.name.text(), "app");
    form_new.derived(LOCAL, "~/proj/app", "the-repo");
    assert_eq!(form_new.name.text(), "the-repo");
    assert!(!form_new.name_edited);

    // Still following: one more character in the folder.
    type_text(&mut form, "2");
    assert_eq!(new(&form).name.text(), "app2");

    // Edited by hand: from then on it stays.
    tab(&mut form, 1);
    assert_eq!(form.focus(), Stop::Name);
    type_text(&mut form, "mine");
    assert_eq!(new(&form).name.text(), "mine");
    assert!(new(&form).name_edited);
    assert_eq!(form.key(&shift(Named::Tab)), FormResult::Pending);
    type_text(&mut form, "/srv/other");
    assert_eq!(new(&form).folder.text(), "/srv/other");
    assert_eq!(new(&form).name.text(), "mine", "an edited name stays");
    let Form::New(form_new) = &mut form else {
        unreachable!()
    };
    assert_eq!(
        form_new.lookup(),
        None,
        "nothing is looked up for an edited name"
    );
    form_new.derived(LOCAL, "/srv/other", "other-repo");
    assert_eq!(form_new.name.text(), "mine");
}

#[test]
fn a_failed_lookup_leaves_the_name() {
    let runner = |_: &[String]| anyhow::bail!("ssh: no route");
    assert_eq!(forms::derived_name("ph", "~/x", LOCAL, &runner), None);
    let empty = |_: &[String]| Ok("\n".to_string());
    assert_eq!(forms::derived_name("ph", "~/x", LOCAL, &empty), None);
}

#[test]
fn invalid_names_block_create_with_a_message() {
    for bad in ["a.b", "a:b", ""] {
        let mut form = new_form(&[]);
        tab(&mut form, 2);
        assert_eq!(form.focus(), Stop::Name);
        if bad.is_empty() {
            form.key(&named(Named::Backspace));
        } else {
            type_text(&mut form, bad);
        }
        assert_eq!(new(&form).name.text(), bad);
        tab(&mut form, 2);
        assert_eq!(form.focus(), Stop::Ok);
        assert_eq!(
            form.key(&named(Named::Enter)),
            FormResult::Pending,
            "{bad:?}"
        );
        let why = valid_session_name(bad).unwrap_err();
        assert_eq!(form.error(), Some(why.as_str()), "{bad:?}");
        assert!(text(&form.panel()).contains(&why), "the form shows why");
        // Enter in the name field is Create too, and is blocked the same way.
        assert_eq!(form.key(&shift(Named::Tab)), FormResult::Pending);
        assert_eq!(form.key(&shift(Named::Tab)), FormResult::Pending);
        assert_eq!(form.focus(), Stop::Name);
        assert_eq!(
            form.key(&named(Named::Enter)),
            FormResult::Pending,
            "{bad:?}"
        );
    }

    let mut form = new_form(&[]);
    tab(&mut form, 2);
    type_text(&mut form, "tmlsrs-test-ok");
    assert_eq!(
        form.key(&named(Named::Enter)),
        FormResult::Submit(Request::New(NewSession {
            host: LOCAL.to_string(),
            folder: "~".to_string(),
            name: "tmlsrs-test-ok".to_string(),
            start: Start::Claude,
            size: None,
        }))
    );
}

#[test]
fn start_lists_shell_claude_then_presets() {
    let form = new_form(&presets());
    let labels: Vec<&str> = new(&form)
        .starts()
        .iter()
        .map(|(label, _)| label.as_str())
        .collect();
    assert_eq!(labels, ["shell", "claude", "opus", "codex"]);
    assert_eq!(new(&form).start, 2, "the first preset is the default");
    assert_eq!(new(&form).starts()[0].1, Start::Shell);
    assert_eq!(new(&form).starts()[1].1, Start::Claude);
    assert_eq!(
        new(&form).starts()[2].1,
        Start::Preset(presets()[0].1.clone())
    );

    let plain = new_form(&[]);
    assert_eq!(new(&plain).starts().len(), 2);
    assert_eq!(new(&plain).start, 1, "claude without presets");

    // What is chosen is what is asked for.
    let mut form = new_form(&presets());
    tab(&mut form, 3);
    form.key(&named(Named::Down));
    tab(&mut form, 1);
    let FormResult::Submit(Request::New(asked)) = form.key(&named(Named::Enter)) else {
        panic!("Create with a valid name submits");
    };
    assert_eq!(asked.start, Start::Preset(vec!["codex".to_string()]));
    assert_eq!(asked.name, "home");

    // A click on an option chooses it.
    let mut form = new_form(&presets());
    let (x, y) = at(&form.panel(), "shell");
    assert_eq!(form.click(x, y), FormResult::Pending);
    assert_eq!(new(&form).start, 0);
    assert_eq!(form.focus(), Stop::Start);
}

#[test]
fn rename_prefills_the_old_name() {
    let old = key("ph", "tmlsrs-test-old");
    let mut form = Form::rename(old.clone());
    let Form::Rename { field, .. } = &form else {
        panic!("a rename form")
    };
    assert_eq!(field.text(), "tmlsrs-test-old");
    assert!(text(&form.panel()).contains("tmlsrs-test-old"));
    // Unchanged: nothing to do.
    assert_eq!(form.key(&named(Named::Enter)), FormResult::Cancelled);

    // The old name is selected: typing replaces it.
    let mut form = Form::rename(old.clone());
    type_text(&mut form, "a.b");
    assert_eq!(form.key(&named(Named::Enter)), FormResult::Pending);
    assert_eq!(
        form.error(),
        Some(valid_session_name("a.b").unwrap_err().as_str())
    );
    for _ in 0..3 {
        form.key(&named(Named::Backspace));
    }
    type_text(&mut form, "tmlsrs-test-new name");
    assert_eq!(
        form.key(&named(Named::Enter)),
        FormResult::Submit(Request::Rename(
            old.clone(),
            "tmlsrs-test-new name".to_string()
        ))
    );

    // End, then more text: the old name is kept and extended.
    let mut form = Form::rename(old.clone());
    form.key(&named(Named::End));
    type_text(&mut form, "2");
    let (x, y) = at(&form.panel(), "Rename");
    assert_eq!(
        form.click(x, y),
        FormResult::Submit(Request::Rename(old, "tmlsrs-test-old2".to_string()))
    );
}

#[test]
fn kill_needs_an_explicit_confirm_and_shows_running_commands() {
    let victim = key("ph", "tmlsrs-test-victim");
    let mut form = Form::confirm_kill(victim.clone());
    assert_eq!(form.focus(), Stop::Cancel, "the default is Cancel");
    assert_eq!(
        form.key(&named(Named::Enter)),
        FormResult::Cancelled,
        "Enter on the default kills nothing"
    );
    let mut form = Form::confirm_kill(victim.clone());
    assert_eq!(
        form.key(&event(Key::Char(' '), Mods::default())),
        FormResult::Cancelled
    );
    // Letters confirm nothing.
    let mut form = Form::confirm_kill(victim.clone());
    type_text(&mut form, "yYk");
    assert_eq!(form.focus(), Stop::Cancel);

    // What a kill would end, through the injected runner: shells are left out.
    let runner = |argv: &[String]| {
        assert_eq!(argv, running_commands_argv(&victim, LOCAL));
        Ok("zsh\nvim\nclaude\nbash\nvim\n".to_string())
    };
    let running = forms::running_commands(&victim, LOCAL, &runner);
    assert_eq!(running, ["claude", "vim"]);
    form.set_running(&key("ph", "another"), vec!["nope".to_string()]);
    form.set_running(&victim, running);
    let said = text(&form.panel());
    assert!(said.contains("Kill tmlsrs-test-victim?"), "{said}");
    assert!(said.contains("Running: claude, vim"), "{said}");
    assert!(!said.contains("nope"), "{said}");

    // Tab to Kill; a held Enter does not confirm, a pressed one does.
    tab(&mut form, 1);
    assert_eq!(form.focus(), Stop::Ok);
    let held = KeyEvent {
        repeat: true,
        ..named(Named::Enter)
    };
    assert_eq!(form.key(&held), FormResult::Pending);
    let released = KeyEvent {
        pressed: false,
        ..named(Named::Enter)
    };
    assert_eq!(form.key(&released), FormResult::Pending);
    assert_eq!(
        form.key(&named(Named::Enter)),
        FormResult::Submit(Request::Kill(victim.clone()))
    );

    // By mouse: Cancel cancels, the Kill button kills, anywhere else does nothing.
    let mut form = Form::confirm_kill(victim.clone());
    let panel = form.panel();
    let (x, y) = at(&panel, "Cancel");
    assert_eq!(form.click(x, y), FormResult::Cancelled);
    assert_eq!(form.click(1.5, 2.5), FormResult::Pending);
    assert_eq!(form.click(-3.0, 900.0), FormResult::Pending);
    let (x, y) = at(&panel, "Kill");
    assert_eq!(form.click(x, y), FormResult::Submit(Request::Kill(victim)));
}

#[test]
fn esc_cancels_every_form() {
    let session = key(LOCAL, "tmlsrs-test-x");
    let forms = [
        new_form(&presets()),
        Form::rename(session.clone()),
        Form::confirm_kill(session.clone()),
    ];
    for mut form in forms {
        // From every stop.
        for stops in 0..6 {
            let mut form = form.clone();
            tab(&mut form, stops);
            assert_eq!(form.key(&named(Named::Escape)), FormResult::Cancelled);
        }
        let (x, y) = at(&form.panel(), "Cancel");
        assert_eq!(form.click(x, y), FormResult::Cancelled);
    }
    let mut menu = RowMenu::new(session);
    assert_eq!(menu.key(&named(Named::Escape)), MenuResult::Closed);
}

#[test]
fn the_row_menu_offers_rename_kill_open_copy() {
    assert_eq!(
        MENU,
        [
            MenuItem::Rename,
            MenuItem::Kill,
            MenuItem::OpenTerminal,
            MenuItem::CopyAttach
        ]
    );
    let session = key("ph", "tmlsrs-test-menu");
    let mut menu = RowMenu::new(session.clone());
    let panel = menu.panel();
    let said = text(&panel);
    for label in [
        "Rename",
        "Kill…",
        "Open in new terminal",
        "Copy attach command",
    ] {
        assert!(said.contains(label), "{label} in {said}");
    }
    // Enter picks the row the arrows are on; the first is Rename, never Kill.
    assert_eq!(
        menu.key(&named(Named::Enter)),
        MenuResult::Pick(MenuItem::Rename)
    );
    assert_eq!(menu.key(&named(Named::Down)), MenuResult::Pending);
    assert_eq!(menu.key(&named(Named::Down)), MenuResult::Pending);
    assert_eq!(
        menu.key(&named(Named::Enter)),
        MenuResult::Pick(MenuItem::OpenTerminal)
    );
    assert_eq!(menu.key(&named(Named::Up)), MenuResult::Pending);
    assert_eq!(
        menu.key(&named(Named::Enter)),
        MenuResult::Pick(MenuItem::Kill)
    );
    // Any other key closes it.
    assert_eq!(
        menu.key(&event(Key::Char('x'), Mods::default())),
        MenuResult::Closed
    );

    let (x, y) = at(&panel, "Copy attach command");
    assert!(menu.hover(x, y));
    assert_eq!(menu.click(x, y), MenuResult::Pick(MenuItem::CopyAttach));
    assert_eq!(
        menu.click(-1.0, 0.5),
        MenuResult::Closed,
        "a click beside it closes"
    );
}

#[test]
fn requests_run_the_tmux_commands_and_say_what_happened() {
    let session = key("ph", "tmlsrs-test-a b'c");
    let create = NewSession {
        host: "ph".to_string(),
        folder: "~/x".to_string(),
        name: "tmlsrs-test-new".to_string(),
        start: Start::Shell,
        size: Some((120, 40)),
    };
    let cases = [
        (
            Request::New(create.clone()),
            new_session_argv(&create, LOCAL),
            "created tmlsrs-test-new on ph",
        ),
        (
            Request::Rename(session.clone(), "tmlsrs-test-b".to_string()),
            rename_argv(&session, "tmlsrs-test-b", LOCAL),
            "renamed tmlsrs-test-a b'c to tmlsrs-test-b on ph",
        ),
        (
            Request::Kill(session.clone()),
            kill_argv(&session, LOCAL),
            "killed tmlsrs-test-a b'c on ph",
        ),
    ];
    for (request, argv, said) in cases {
        assert_eq!(forms::request_argv(&request, LOCAL), argv);
        let ran = RefCell::new(Vec::new());
        let ok = |argv: &[String]| {
            ran.borrow_mut().push(argv.to_vec());
            Ok(String::new())
        };
        assert_eq!(forms::perform(&request, LOCAL, &ok), Ok(said.to_string()));
        assert_eq!(*ran.borrow(), [argv], "one command, the builder's");

        let failing = |_: &[String]| anyhow::bail!("a session named x already exists\n");
        let error = forms::perform(&request, LOCAL, &failing).unwrap_err();
        assert!(
            error.ends_with(": a session named x already exists"),
            "one line with the reason: {error:?}"
        );
        assert!(!error.contains('\n'));
    }
}

#[test]
fn open_in_new_terminal_starts_the_users_terminal_with_the_attach_command() {
    let session = key("ph", "tmlsrs-test-a b'c");
    let attach = attach_argv(&session, LOCAL);
    let with = |terminal: Option<&str>, kitty: bool| {
        forms::terminal_argv(terminal, &|program| kitty && program == "kitty", &attach)
    };
    let mut expected = vec!["foot".to_string(), "-e".to_string()];
    expected.extend(attach.clone());
    assert_eq!(with(Some("foot"), true), expected, "$TERMINAL first");
    expected[0] = "x-terminal-emulator".to_string();
    assert_eq!(with(None, false), expected, "the Debian alternative last");
    assert_eq!(
        with(Some(""), false),
        expected,
        "an empty $TERMINAL is none"
    );
    let mut kitty = vec!["kitty".to_string()];
    kitty.extend(attach.clone());
    assert_eq!(with(None, true), kitty, "kitty takes the command as it is");
    assert_eq!(with(Some("kitty"), false), kitty);

    // Nothing is started here: the spawner is the test's.
    let mut started = Vec::new();
    let said = forms::open_terminal(&session, LOCAL, Some("foot"), &|_| false, &mut |argv| {
        started.push(argv.to_vec());
        Ok(())
    });
    assert_eq!(said, Ok("opened tmlsrs-test-a b'c in foot".to_string()));
    assert_eq!(started.len(), 1);
    assert_eq!(started[0][..2], ["foot".to_string(), "-e".to_string()]);
    assert_eq!(started[0][2..], attach[..]);

    let said = forms::open_terminal(&session, LOCAL, None, &|_| false, &mut |_| {
        Err(std::io::Error::from(std::io::ErrorKind::NotFound))
    });
    let error = said.unwrap_err();
    assert!(
        error.starts_with("couldn't start x-terminal-emulator"),
        "{error}"
    );
}

#[test]
fn copy_puts_the_attach_command_on_the_clipboard() {
    let session = key("ph", "tmlsrs-test-a b'c");
    let mut copied = Vec::new();
    let said = forms::copy_attach(&session, LOCAL, &mut |text| {
        copied.push(text);
        Ok(())
    });
    assert_eq!(copied, [attach_text(&session, LOCAL)]);
    assert_eq!(
        said,
        Ok("copied the attach command of tmlsrs-test-a b'c".to_string())
    );
    let said = forms::copy_attach(&session, LOCAL, &mut |_| Err("no clipboard".to_string()));
    assert_eq!(said, Err("copy failed: no clipboard".to_string()));
}

/// No text runs out of its panel and no two pieces share a cell, whatever the names are.
#[test]
fn panels_keep_their_text_inside_and_apart() {
    let long = "tmlsrs-test-".to_string() + &"long name ".repeat(12);
    let session = key("a-host-with-quite-a-long-name.example.org", &long);
    let many: Vec<(String, Vec<String>)> = (0..6)
        .map(|n| (format!("preset number {n} {long}"), vec!["x".to_string()]))
        .collect();
    let mut new = Form::new_session(std::slice::from_ref(&session.host), &session.host, &many);
    tab(&mut new, 1);
    type_text(&mut new, &format!("~/{long}"));
    tab(&mut new, 3);
    new.key(&named(Named::Enter));
    let mut kill = Form::confirm_kill(session.clone());
    kill.set_running(&session, (0..30).map(|n| format!("command-{n}")).collect());
    let panels = [
        new.panel(),
        Form::rename(session.clone()).panel(),
        kill.panel(),
        RowMenu::new(session).panel(),
    ];
    for panel in panels {
        let mut taken = vec![vec![false; panel.cols]; panel.lines];
        for cell in &panel.cells {
            assert!(cell.line < panel.lines, "{cell:?} is under the panel");
            assert!(
                cell.col + cell.cols <= panel.cols,
                "{cell:?} runs out of the panel"
            );
            let shown: usize = cell.text.chars().count();
            assert!(
                shown <= cell.cols,
                "{cell:?} says more than it has room for"
            );
            for taken in &mut taken[cell.line][cell.col..cell.col + cell.cols] {
                assert!(!*taken, "{cell:?} overlaps another");
                *taken = true;
            }
        }
        assert!(panel.cells.iter().any(|cell| cell.look == Look::Title));
    }
}

/// The sizes the form's request is given are the caller's: the form leaves `size` open.
#[test]
fn the_new_session_request_is_sized_by_the_caller() {
    let mut form = new_form(&[]);
    let FormResult::Submit(Request::New(mut asked)) = form.key(&named(Named::Enter)) else {
        panic!("Enter on the host creates with the defaults");
    };
    assert_eq!(asked.size, None);
    asked.size = Some((132, 43));
    let argv = tmux::new_session_argv(&asked, LOCAL);
    assert!(argv.last().unwrap().contains("-x 132 -y 43"), "{argv:?}");
}

const DIALOG: &str = include_str!("../../client-core/tests/data/approve-dialog.txt");

/// A listed Claude session (`pane` from its status file).
fn info(name: &str, status: Status, reason: Option<&str>) -> SessionInfo {
    SessionInfo {
        key: key(LOCAL, name),
        status,
        reason: reason.map(str::to_string),
        tool: Tool::Claude,
        title: None,
        context_pct: None,
        attached: 0,
        last_output_age_s: None,
        pid: None,
        session_id: None,
        pane: Some("%3".to_string()),
        activity: None,
    }
}

#[test]
fn answer_sends_the_key_only_while_the_same_dialog_is_up() {
    let calls = RefCell::new(Vec::new());
    let screen = RefCell::new(DIALOG.to_string());
    let run = |argv: &[String]| -> anyhow::Result<String> {
        calls.borrow_mut().push(argv.to_vec());
        Ok(screen.borrow().clone())
    };
    let k = key(LOCAL, "w");
    let shown = forms::approval(&k, Some("%7"), LOCAL, &run).unwrap();
    assert_eq!(
        shown,
        ["Bash command", "Create empty marker file", "touch /tmp/x"]
    );
    assert_eq!(
        forms::answer(&k, Some("%7"), &shown, true, LOCAL, &run),
        Ok("approved w".to_string())
    );
    assert_eq!(
        calls.borrow().last(),
        Some(&tmux::send_keys_argv(&k, Some("%7"), "1", LOCAL))
    );
    assert_eq!(
        forms::answer(&k, Some("%7"), &shown, false, LOCAL, &run),
        Ok("declined w".to_string())
    );
    assert_eq!(
        calls.borrow().last(),
        Some(&tmux::send_keys_argv(&k, Some("%7"), "Escape", LOCAL))
    );
    // Claude moved on — another dialog, or none: nothing is sent.
    for now in [
        DIALOG.replace("touch /tmp/x", "rm -rf /tmp/y"),
        "❯ \n".to_string(),
    ] {
        *screen.borrow_mut() = now;
        calls.borrow_mut().clear();
        assert_eq!(
            forms::answer(&k, Some("%7"), &shown, true, LOCAL, &run),
            Err("w isn't asking that any more".to_string())
        );
        assert_eq!(
            *calls.borrow(),
            [tmux::capture_argv(&k, Some("%7"), LOCAL)],
            "only the capture ran"
        );
    }
    // Nothing asks: the form never opens.
    assert_eq!(
        forms::approval(&k, None, LOCAL, &run),
        Err("w isn't asking anything".to_string())
    );
    let down = |_: &[String]| -> anyhow::Result<String> {
        anyhow::bail!("ssh: connect to host ph: timed out")
    };
    assert_eq!(
        forms::approval(&k, None, LOCAL, &down),
        Err("couldn't read w: ssh: connect to host ph: timed out".to_string())
    );
}

#[test]
fn the_approve_form_answers_yes_on_enter_no_on_n_and_cancels_on_esc() {
    let k = key(LOCAL, "w");
    let dialog: Vec<String> = ["Bash command", "touch /tmp/x"]
        .map(str::to_string)
        .to_vec();
    let answer = |yes: bool| {
        FormResult::Submit(Request::Answer {
            key: k.clone(),
            pane: Some("%3".to_string()),
            shown: dialog.clone(),
            yes,
        })
    };
    let form = || Form::approve(k.clone(), Some("%3".to_string()), dialog.clone());
    assert_eq!(form().focus(), Stop::Ok);
    assert_eq!(form().key(&named(Named::Enter)), answer(true));
    assert_eq!(
        form().key(&event(Key::Char('n'), Mods::default())),
        answer(false)
    );
    assert_eq!(form().key(&named(Named::Escape)), FormResult::Cancelled);
    // A held Enter answers nothing: it may be the one that opened the form.
    let mut held = named(Named::Enter);
    held.repeat = true;
    assert_eq!(form().key(&held), FormResult::Pending);
    // Tab: Yes, No, Cancel.
    let mut f = form();
    f.key(&named(Named::Tab));
    assert_eq!(f.focus(), Stop::No);
    assert_eq!(f.key(&named(Named::Enter)), answer(false));
    // The panel shows the dialog and the three buttons.
    let texts: Vec<String> = form()
        .panel()
        .cells
        .iter()
        .map(|cell| cell.text.clone())
        .collect();
    for want in [
        "here · w asks",
        "Bash command",
        "touch /tmp/x",
        " Yes ",
        " No ",
        " Cancel ",
    ] {
        assert!(texts.iter().any(|text| text == want), "{want}: {texts:?}");
    }
}

#[test]
fn menu_items_offer_approve_only_while_claude_asks() {
    assert_eq!(forms::menu_items(None), MENU.to_vec());
    assert_eq!(
        forms::menu_items(Some(&info("w", Status::Idle, None))),
        MENU.to_vec()
    );
    assert_eq!(
        forms::menu_items(Some(&info("w", Status::Waiting, Some("input needed")))),
        MENU.to_vec()
    );
    let asks = info("w", Status::Waiting, Some("permission prompt"));
    assert_eq!(
        forms::menu_items(Some(&asks)),
        [
            MenuItem::Approve,
            MenuItem::Rename,
            MenuItem::Kill,
            MenuItem::OpenTerminal,
            MenuItem::CopyAttach
        ]
    );
    let mut menu = RowMenu::with(asks.key.clone(), forms::menu_items(Some(&asks)));
    assert!(
        menu.panel()
            .cells
            .iter()
            .any(|cell| cell.text == " Approve…")
    );
    assert_eq!(
        menu.key(&named(Named::Enter)),
        MenuResult::Pick(MenuItem::Approve)
    );
    assert_eq!(RowMenu::new(key(LOCAL, "w")).items(), MENU);
}

#[test]
fn a_dialog_that_comes_back_after_the_user_moved_on_opens_no_form() {
    use forms::ApprovalFate::{MovedOn, Open, Superseded};
    // Request 3 is the latest, no key since its `y` (5 presses then and now), focus unchanged.
    assert_eq!(forms::approval_fate(3, 3, 5, 5, true), Open);
    // A newer `y` asked again: this answer is dropped, the newer one decides.
    assert_eq!(forms::approval_fate(2, 3, 5, 5, true), Superseded);
    assert_eq!(forms::approval_fate(2, 3, 5, 9, false), Superseded);
    // A key pressed since (Esc back to the terminal, a letter, an Enter for the shell): the
    // form would take it, and an Enter on Yes would approve.
    assert_eq!(forms::approval_fate(3, 3, 5, 6, true), MovedOn);
    // The focus left the list or the alerts (a click on the terminal, a menu opened).
    assert_eq!(forms::approval_fate(3, 3, 5, 5, false), MovedOn);
}
