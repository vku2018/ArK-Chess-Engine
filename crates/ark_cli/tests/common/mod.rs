use serde_json::Value;

pub fn parse_single_json(stdout: Vec<u8>) -> Result<Value, Box<dyn std::error::Error>> {
    let stdout = String::from_utf8(stdout)?;
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    Ok(serde_json::from_str(&stdout)?)
}
