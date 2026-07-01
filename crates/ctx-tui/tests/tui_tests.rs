use std::io::Cursor;
use ctx_tui::run_interactive_menu;

#[test]
fn test_tui_exit_immediately() {
    let input = b"7\n";
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();

    run_interactive_menu(&mut reader, &mut writer).unwrap();

    let output = String::from_utf8(writer).unwrap();
    assert!(output.contains("=== CTX Terminal Interactive Menu ==="));
    assert!(output.contains("Goodbye!"));
}

#[test]
fn test_tui_change_path_and_exit() {
    let input = b"1\n/my/custom/path\n7\n";
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();

    run_interactive_menu(&mut reader, &mut writer).unwrap();

    let output = String::from_utf8(writer).unwrap();
    assert!(output.contains("Enter new scan path:"));
    assert!(output.contains("1. Set scan path (current: /my/custom/path)"));
    assert!(output.contains("Goodbye!"));
}

#[test]
fn test_tui_change_mode_and_exit() {
    let input = b"2\n3\n7\n"; // 2 (Set mode) -> 3 (Code) -> 7 (Exit)
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();

    run_interactive_menu(&mut reader, &mut writer).unwrap();

    let output = String::from_utf8(writer).unwrap();
    assert!(output.contains("Select mode:"));
    assert!(output.contains("2. Set scan mode (current: code)"));
    assert!(output.contains("Goodbye!"));
}

#[test]
fn test_tui_change_format_and_exit() {
    let input = b"3\n2\n7\n"; // 3 (Set format) -> 2 (XML) -> 7 (Exit)
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();

    run_interactive_menu(&mut reader, &mut writer).unwrap();

    let output = String::from_utf8(writer).unwrap();
    assert!(output.contains("Select format:"));
    assert!(output.contains("3. Set output format (current: xml)"));
    assert!(output.contains("Goodbye!"));
}

#[test]
fn test_tui_change_depth_and_exit() {
    let input = b"4\n12\n7\n"; // 4 (Set depth) -> 12 -> 7 (Exit)
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();

    run_interactive_menu(&mut reader, &mut writer).unwrap();

    let output = String::from_utf8(writer).unwrap();
    assert!(output.contains("Enter max depth (leave blank for None):"));
    assert!(output.contains("4. Set max depth (current: 12)"));
    assert!(output.contains("Goodbye!"));
}

#[test]
fn test_tui_change_size_and_exit() {
    let input = b"5\n1024\n7\n"; // 5 (Set file size) -> 1024 (KB) -> 7 (Exit)
    let mut reader = Cursor::new(input);
    let mut writer = Vec::new();

    run_interactive_menu(&mut reader, &mut writer).unwrap();

    let output = String::from_utf8(writer).unwrap();
    assert!(output.contains("Enter max file size in KB:"));
    assert!(output.contains("5. Set max file size (current: 1024 KB)"));
    assert!(output.contains("Goodbye!"));
}
