use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Recording,
    Transcribing,
    Pasting,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            State::Idle => write!(f, "Idle"),
            State::Recording => write!(f, "Listening..."),
            State::Transcribing => write!(f, "Transcribing..."),
            State::Pasting => write!(f, "Pasting"),
        }
    }
}

impl State {
    pub fn on_key_down(self) -> Self {
        match self {
            State::Idle => State::Recording,
            other => other,
        }
    }

    pub fn on_key_up(self) -> Self {
        match self {
            State::Recording => State::Transcribing,
            other => other,
        }
    }

    pub fn on_transcription_done(self) -> Self {
        match self {
            State::Transcribing => State::Pasting,
            other => other,
        }
    }

    pub fn on_paste_done(self) -> Self {
        match self {
            State::Pasting => State::Idle,
            other => other,
        }
    }
}
