#[allow(dead_code)]
#[path = "../src/json.rs"]
mod json;

#[test]
fn json_numbers_follow_the_grammar_not_rust_float_syntax() {
    for bad in [
        "01", "-01", "1.", "-.1", "00", "1.e2", "1e", "1e+", "+1", ".1", "--1", "1e+-2",
    ] {
        assert!(
            json::parse(bad).is_err(),
            "accepted invalid JSON number {bad}"
        );
        assert!(
            json::parse(&format!("{{\"x\":{bad}}}")).is_err(),
            "accepted nested {bad}"
        );
    }
    for good in [
        "0",
        "-0",
        "1.0",
        "-0.1",
        "1e2",
        "1E+2",
        "1e-2",
        "9007199254740992",
        "1e-300",
    ] {
        assert!(json::parse(good).is_ok(), "rejected {good}");
    }
}

#[test]
fn json_strings_reject_all_unescaped_control_characters() {
    for c in 0..32u8 {
        assert!(
            json::parse(&format!("\"a{}b\"", c as char)).is_err(),
            "accepted control {c}"
        );
        assert!(json::parse(&format!("\"a\\u{c:04x}b\"")).is_ok());
    }
    for good in [r#""😀""#, r#""\ud83d\ude00""#, r#""\n\t\r\b\f\/\\\"""#] {
        assert!(json::parse(good).is_ok());
    }
}
