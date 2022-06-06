use std::{
    collections::HashMap,
    io::{Read, Result as IOResult, Write},
    net::TcpStream,
    thread,
    time::Duration,
};

use fancy_regex::Regex;

use self::parser::Parser;

mod parser;

const IRC_PORT: u16 = 6667;
const IRC_URL: &str = "irc.chat.twitch.tv";


#[derive(Debug)]
/// Error types
pub enum IrcError {
    Timeout,
    Host(String),
    MaxAttemps,
    Permission,
    Aborted,
    Unknown,
}

/// Return a Irc Object or an IrcError
pub type IrcResult = Result<Irc, IrcError>;

impl From<std::io::Error> for IrcError {
    fn from(err: std::io::Error) -> Self {
        use std::io::ErrorKind;
        match err.kind() {
            ErrorKind::ConnectionReset => Self::Host("connection reset by peer".into()),
            ErrorKind::ConnectionRefused => Self::Host("connection refused by host".into()),
            ErrorKind::NotFound => Self::Host("unknown host".into()),
            ErrorKind::PermissionDenied => Self::Permission,
            ErrorKind::ConnectionAborted => Self::Aborted,
            ErrorKind::BrokenPipe => Self::Host("broken pipe".into()),
            _ => Self::Unknown,
        }
    }
}

/// IRC Commands
pub enum Command {
    /// Account OAuth Pass
    Pass, 
    /// Account nickname
    Nick, 
    /// Join a Channel
    Join, 
    /// Pong a ping
    Pong, 
    /// Ping IRC Twitch Chat
    Ping, 
    /// Send chat message
    Privmsg 
}

impl Command {
    pub fn build<T>(&self, arg: String, connection: &LocoConnection<T>) -> String
    where
        T: Read + Write + Unpin,
    {
        let prefix = match self {
            Self::Pass => "PASS oauth:".into(),
            Self::Nick => "NICK ".into(),
            Self::Join => "JOIN #".into(),
            Self::Pong => "PONG :tmi.twitch.tv".into(),
            Self::Ping => "PING".into(),
            Self::Privmsg => format!("PRIVMSG #{} :", connection.config.channel_to_join.clone()),
        };
        format!("{}{}\r\n", prefix, &arg)
    }
}


/// Connection T is only for a mock in tests, 
/// Use new method instead
pub struct LocoConnection<T>
where
    T: Read + Write + Unpin,
{
    connection: Option<T>,
    config: LocoConfig,
}


/// Configuration of authentication in IRC Twitch Chat
#[derive(Clone)]
pub struct LocoConfig {
    oauth: String,
    nickname: String,
    channel_to_join: String,
}

/// IRC event
#[derive(Debug)]
pub struct Irc {
    /// Type of IRC Event
    pub irc_type: IrcType,
    /// Only have nickname in event
    pub nickname: Option<String>,
    /// Message if as PRIVMSG event
    pub keys: Option<HashMap<String, String>>,
    /// Channel of event
    pub channel: String,
    /// Message if as PRIVMSG event
    pub message: Option<String>,
}

impl Irc {
    pub fn new(
        irc_type: IrcType,
        nickname: Option<String>,
        keys: Option<HashMap<String, String>>,
        channel: String,
        message: Option<String>,
    ) -> Self {
        Self {
            irc_type,
            nickname,
            keys,
            channel,
            message,
        }
    }
}

#[derive(Debug)]
pub enum IrcType {
    Message,
    Join,
    Part,
    Usernotice,
    CleanChat,
    Pong,
    Ping,
    UserState,
    Notice,
    Unknown,
}

impl IrcType {
    #[doc(hidden)]
    fn display_name(&self) -> Regex {
        let expr = match self {
            Self::Message => r"(?<=:)(\w+)(?=!)",
            Self::Join => r"(?<=:)(\w+)(?=!)",
            Self::Part => r"(?<=:)(\w+)(?=!)",
            Self::Usernotice => r"(?<=display-name=)([\w]+)",
            Self::CleanChat => r"(?<=:)([\w]+)(?!.)",
            Self::Pong => r"TODOU",
            Self::Ping => r"TODOU",
            Self::UserState => r"(?<=display-name=)([\w]+)",
            Self::Notice => r"TODOU",
            _ => r"TODOU",
        };
        Regex::new(expr).unwrap()
    }
}

#[doc(hidden)]
impl From<String> for IrcType {
    fn from(value: String) -> Self {
        match &value[..] {
            "PRIVMSG" => Self::Message,
            "JOIN" => Self::Join,
            "PART" => Self::Part,
            "USERNOTICE" => Self::Usernotice,
            "CLEARCHAT" => Self::CleanChat,
            "PING" => Self::Ping,
            "PONG" => Self::Pong,
            "NOTICE" => Self::Notice,
            _ => Self::Unknown,
        }
    }
}

impl LocoConfig {
    /// Returns a Config Object
    pub fn new(oauth: String, nickname: String, channel_to_join: String) -> Self {
        Self {
            oauth,
            nickname,
            channel_to_join,
        }
    }
}

impl LocoConnection<TcpStream> {
    /// Initialize a Tcp Connection
    pub fn new(loco_config: LocoConfig) -> Result<LocoConnection<TcpStream>, IrcError> {
        let con: LocoConnection<TcpStream> = LocoConnection::try_connect(loco_config)?;
        Ok(con)
    }

    fn try_connect(loco_config: LocoConfig) -> Result<LocoConnection<TcpStream>, IrcError> {
        const MAX_ATTEMPS: usize = 3;
        for attempt in 0..MAX_ATTEMPS {
            println!("connection attempt {att}", att = attempt + 1);
            match TcpStream::connect(&format!("{}:{}", IRC_URL, IRC_PORT)) {
                Ok(connection) => {
                    let mut loco_connection = LocoConnection {
                        connection: Some(connection),
                        config: loco_config.clone(),
                    };
                    loco_connection.batch_command(&[
                        Command::Pass.build(loco_config.oauth.clone(), &loco_connection),
                        Command::Nick.build(loco_config.nickname.clone(), &loco_connection),
                        Command::Join.build(loco_config.channel_to_join, &loco_connection),
                        "CAP REQ :twitch.tv/commands\r\n".into(),
                        "CAP REQ :twitch.tv/membership\r\n".into(),
                        "CAP REQ :twitch.tv/tags\r\n".into(),
                    ])?;
                    return Ok(loco_connection);
                }
                _ => {
                    if attempt == MAX_ATTEMPS {
                        return Err(IrcError::MaxAttemps);
                    }
                    thread::sleep(Duration::from_secs((2_u64).pow(attempt as u32)))
                }
            }
        }
        Err(IrcError::Unknown)
    }

    fn batch_command(&mut self, vec: &[String]) -> IOResult<()> {
        let map = vec.iter().flat_map(|val| val.bytes()).collect::<Vec<u8>>();
        if let Some(connection) = &mut self.connection {
            connection.write_all(&map)?;
        }
        Ok(())
    }

    /// Send a command to IRC
    pub fn send_command(&mut self, command: Command, arg: &str) -> IOResult<()> {
        let command = command.build(arg.into(), self);
        self.batch_command(&[command])?;
        Ok(())
    }

    /// Another way to handle messages, but cannot send commands with the same connection
    //TODO: greceful shutdown
    pub fn read(&mut self, exec: impl Fn(Irc)) {
        for irc in self {
            exec(irc)
        }
    }
}

impl<T> Iterator for LocoConnection<T>
where
    T: Read + Write + Unpin,
{
    type Item = Irc;

    fn next(&mut self) -> Option<Self::Item> {
        let mut irc: Option<Self::Item> = None;
        if let Some(connection) = &mut self.connection {
            let mut buf = [0; 1024];
            if connection.read(&mut buf).is_ok() {
                if let Ok(msg) = String::from_utf8(Vec::from(buf)) {
                    if let Ok(value) = Parser.parse(msg) {
                        irc = Some(value);
                    }
                }
            }
        }
        irc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_commands() {
        let fake_conn: LocoConnection<TcpStream> = LocoConnection {
            connection: None,
            config: LocoConfig {
                oauth: "test".into(),
                nickname: "test".into(),
                channel_to_join: "test".into(),
            },
        };
        let inputs = [
            (Command::Join, "test", "JOIN #test\r\n"),
            (Command::Nick, "test", "NICK test\r\n"),
            (Command::Privmsg, "test", "PRIVMSG #test :test\r\n"),
            (Command::Pass, "test", "PASS oauth:test\r\n"),
        ];

        for (command, param, expected) in inputs {
            assert_eq!(expected, command.build(param.into(), &fake_conn))
        }
    }
}
