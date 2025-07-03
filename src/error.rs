use std::fmt::Display;

pub fn err<T>(message: impl ToString) -> Result<T, Error> {
    Err(Error {
        message: message.to_string(),
    })
}

#[derive(Debug, Clone)]
pub struct Error {
    pub message: String,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}
