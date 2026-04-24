pub enum Command {
    Search(String),
    Explain(String),
}

pub fn parse_request(input: &str) -> Option<Command> {
    let (kind, value) = input.split_once(':')?;
    match kind.trim() {
        "search" => Some(Command::Search(value.trim().to_owned())),
        "explain" => Some(Command::Explain(value.trim().to_owned())),
        _ => None,
    }
}
