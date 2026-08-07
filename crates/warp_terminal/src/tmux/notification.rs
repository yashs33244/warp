//! Line-level tmux control-mode parser.
//!
//! `parse_line` classifies a single control line (newline already stripped)
//! into a `TmuxEvent`. `ControlParser` is a stateful layer that assembles
//! `%begin`/`%end`/`%error` reply blocks into `CommandReply` values.
//!
//! Output payloads are byte-oriented (they may be non-UTF-8); every other
//! field is ASCII. No input can make either layer panic.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxEvent {
    Begin {
        time: i64,
        num: u64,
        flags: i64,
    },
    End {
        time: i64,
        num: u64,
        flags: i64,
    },
    Error {
        time: i64,
        num: u64,
        flags: i64,
    },
    Output {
        pane: u32,
        data: Vec<u8>,
    },
    ExtendedOutput {
        pane: u32,
        age: u64,
        data: Vec<u8>,
    },
    Pause {
        pane: u32,
    },
    Continue {
        pane: u32,
    },
    WindowAdd {
        window: u32,
    },
    WindowClose {
        window: u32,
    },
    WindowRenamed {
        window: u32,
        name: String,
    },
    WindowPaneChanged {
        window: u32,
        pane: u32,
    },
    LayoutChange {
        window: u32,
        layout: String,
        visible: Option<String>,
        flags: Option<String>,
    },
    SessionChanged {
        session: u32,
        name: String,
    },
    SessionsChanged,
    SessionWindowChanged {
        session: u32,
        window: u32,
    },
    SessionRenamed {
        session: u32,
        name: String,
    },
    PaneModeChanged {
        pane: u32,
    },
    ClientSessionChanged {
        client: String,
        session: u32,
        name: String,
    },
    ClientDetached {
        client: String,
    },
    Exit {
        reason: Option<String>,
    },
    Other(Vec<u8>),
}

fn split_first_space(line: &[u8]) -> (&[u8], Option<&[u8]>) {
    match line.iter().position(|&b| b == b' ') {
        Some(i) => (&line[..i], Some(&line[i + 1..])),
        None => (line, None),
    }
}

fn split_spaces(b: &[u8]) -> Vec<&[u8]> {
    b.split(|&c| c == b' ').collect()
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn parse_u32(b: &[u8]) -> Option<u32> {
    if b.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    for &c in b {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(u32::from(c - b'0'))?;
    }
    Some(v)
}

fn parse_u64(b: &[u8]) -> Option<u64> {
    if b.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for &c in b {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(u64::from(c - b'0'))?;
    }
    Some(v)
}

fn parse_i64(b: &[u8]) -> Option<i64> {
    if b.is_empty() {
        return None;
    }
    let (neg, digits) = if b[0] == b'-' {
        (true, &b[1..])
    } else {
        (false, b)
    };
    if digits.is_empty() {
        return None;
    }
    let mut v: i64 = 0;
    for &c in digits {
        if !c.is_ascii_digit() {
            return None;
        }
        let d = i64::from(c - b'0');
        v = v.checked_mul(10)?;
        v = if neg {
            v.checked_sub(d)?
        } else {
            v.checked_add(d)?
        };
    }
    Some(v)
}

fn strip_sigil(tok: &[u8], sigil: u8) -> Option<u32> {
    let rest = tok.strip_prefix(&[sigil])?;
    parse_u32(rest)
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn single_sigil(rest: Option<&[u8]>, sigil: u8) -> Option<u32> {
    strip_sigil(rest?, sigil)
}

/// Parse a single control line into a `TmuxEvent`.
///
/// Any line that does not start with `%`, or is otherwise unrecognized or
/// malformed, becomes `TmuxEvent::Other(raw)`.
pub fn parse_line(line: &[u8]) -> TmuxEvent {
    let other = || TmuxEvent::Other(line.to_vec());
    if line.first() != Some(&b'%') {
        return other();
    }
    let (kw_bytes, rest) = split_first_space(line);
    let kw = match std::str::from_utf8(kw_bytes) {
        Ok(k) => k,
        Err(_) => return other(),
    };

    match kw {
        "%begin" | "%end" | "%error" => {
            let r = match rest {
                Some(r) => r,
                None => return other(),
            };
            let toks = split_spaces(r);
            if toks.len() != 3 {
                return other();
            }
            let time = match parse_i64(toks[0]) {
                Some(v) => v,
                None => return other(),
            };
            let num = match parse_u64(toks[1]) {
                Some(v) => v,
                None => return other(),
            };
            let flags = match parse_i64(toks[2]) {
                Some(v) => v,
                None => return other(),
            };
            match kw {
                "%begin" => TmuxEvent::Begin { time, num, flags },
                "%end" => TmuxEvent::End { time, num, flags },
                _ => TmuxEvent::Error { time, num, flags },
            }
        }

        "%output" => {
            let r = match rest {
                Some(r) => r,
                None => return other(),
            };
            let (pane_tok, data) = split_first_space(r);
            let pane = match strip_sigil(pane_tok, b'%') {
                Some(p) => p,
                None => return other(),
            };
            TmuxEvent::Output {
                pane,
                data: super::escape::unescape(data.unwrap_or(b"")),
            }
        }

        "%extended-output" => {
            let r = match rest {
                Some(r) => r,
                None => return other(),
            };
            let (pane_tok, rem) = split_first_space(r);
            let pane = match strip_sigil(pane_tok, b'%') {
                Some(p) => p,
                None => return other(),
            };
            let rem = match rem {
                Some(x) => x,
                None => return other(),
            };
            let sep = match find_subslice(rem, b" : ") {
                Some(i) => i,
                None => return other(),
            };
            let age = match parse_u64(&rem[..sep]) {
                Some(v) => v,
                None => return other(),
            };
            TmuxEvent::ExtendedOutput {
                pane,
                age,
                data: super::escape::unescape(&rem[sep + 3..]),
            }
        }

        "%pause" => single_sigil(rest, b'%').map_or_else(other, |pane| TmuxEvent::Pause { pane }),
        "%continue" => {
            single_sigil(rest, b'%').map_or_else(other, |pane| TmuxEvent::Continue { pane })
        }
        "%pane-mode-changed" => {
            single_sigil(rest, b'%').map_or_else(other, |pane| TmuxEvent::PaneModeChanged { pane })
        }
        "%window-add" => {
            single_sigil(rest, b'@').map_or_else(other, |window| TmuxEvent::WindowAdd { window })
        }
        "%window-close" => {
            single_sigil(rest, b'@').map_or_else(other, |window| TmuxEvent::WindowClose { window })
        }

        "%window-renamed" => {
            let r = match rest {
                Some(r) => r,
                None => return other(),
            };
            let (wtok, name) = split_first_space(r);
            let window = match strip_sigil(wtok, b'@') {
                Some(w) => w,
                None => return other(),
            };
            match name {
                Some(n) => TmuxEvent::WindowRenamed {
                    window,
                    name: lossy(n),
                },
                None => other(),
            }
        }

        "%window-pane-changed" => {
            let r = match rest {
                Some(r) => r,
                None => return other(),
            };
            let toks = split_spaces(r);
            if toks.len() != 2 {
                return other();
            }
            let window = match strip_sigil(toks[0], b'@') {
                Some(w) => w,
                None => return other(),
            };
            let pane = match strip_sigil(toks[1], b'%') {
                Some(p) => p,
                None => return other(),
            };
            TmuxEvent::WindowPaneChanged { window, pane }
        }

        "%layout-change" => {
            let r = match rest {
                Some(r) => r,
                None => return other(),
            };
            let toks = split_spaces(r);
            if !(2..=4).contains(&toks.len()) {
                return other();
            }
            let window = match strip_sigil(toks[0], b'@') {
                Some(w) => w,
                None => return other(),
            };
            TmuxEvent::LayoutChange {
                window,
                layout: lossy(toks[1]),
                visible: toks.get(2).map(|t| lossy(t)),
                flags: toks.get(3).map(|t| lossy(t)),
            }
        }

        "%session-changed" => {
            let r = match rest {
                Some(r) => r,
                None => return other(),
            };
            let (stok, name) = split_first_space(r);
            let session = match strip_sigil(stok, b'$') {
                Some(s) => s,
                None => return other(),
            };
            match name {
                Some(n) => TmuxEvent::SessionChanged {
                    session,
                    name: lossy(n),
                },
                None => other(),
            }
        }

        "%session-renamed" => {
            let r = match rest {
                Some(r) => r,
                None => return other(),
            };
            let (stok, name) = split_first_space(r);
            let session = match strip_sigil(stok, b'$') {
                Some(s) => s,
                None => return other(),
            };
            match name {
                Some(n) => TmuxEvent::SessionRenamed {
                    session,
                    name: lossy(n),
                },
                None => other(),
            }
        }

        "%sessions-changed" => {
            if rest.is_none() {
                TmuxEvent::SessionsChanged
            } else {
                other()
            }
        }

        "%session-window-changed" => {
            let r = match rest {
                Some(r) => r,
                None => return other(),
            };
            let toks = split_spaces(r);
            if toks.len() != 2 {
                return other();
            }
            let session = match strip_sigil(toks[0], b'$') {
                Some(s) => s,
                None => return other(),
            };
            let window = match strip_sigil(toks[1], b'@') {
                Some(w) => w,
                None => return other(),
            };
            TmuxEvent::SessionWindowChanged { session, window }
        }

        "%client-session-changed" => {
            let r = match rest {
                Some(r) => r,
                None => return other(),
            };
            let (client, rem) = split_first_space(r);
            let rem = match rem {
                Some(x) => x,
                None => return other(),
            };
            let (stok, name) = split_first_space(rem);
            let session = match strip_sigil(stok, b'$') {
                Some(s) => s,
                None => return other(),
            };
            match name {
                Some(n) => TmuxEvent::ClientSessionChanged {
                    client: lossy(client),
                    session,
                    name: lossy(n),
                },
                None => other(),
            }
        }

        "%client-detached" => match rest {
            Some(r) if !r.is_empty() => TmuxEvent::ClientDetached { client: lossy(r) },
            _ => other(),
        },

        "%exit" => match rest {
            Some(r) if !r.is_empty() => TmuxEvent::Exit {
                reason: Some(lossy(r)),
            },
            _ => TmuxEvent::Exit { reason: None },
        },

        _ => other(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlMessage {
    Notification(TmuxEvent),
    Reply(CommandReply),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandReply {
    pub num: u64,
    pub success: bool,
    pub body: Vec<Vec<u8>>,
}

struct Block {
    num: u64,
    body: Vec<Vec<u8>>,
}

/// Stateful assembler for `%begin`/`%end`/`%error` reply blocks.
///
/// Feed one control line at a time. Outside a block, lines are classified with
/// `parse_line` and returned as notifications; a `%begin` opens a block whose
/// subsequent raw lines are collected as the reply body until the matching
/// `%end` (success) or `%error` (failure) with the same command number closes it.
#[derive(Default)]
pub struct ControlParser {
    block: Option<Block>,
}

impl ControlParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed_line(&mut self, line: &[u8]) -> Option<ControlMessage> {
        if self.block.is_some() {
            let bnum = self.block.as_ref().unwrap().num;
            match parse_line(line) {
                TmuxEvent::End { num, .. } if num == bnum => {
                    let b = self.block.take().unwrap();
                    Some(ControlMessage::Reply(CommandReply {
                        num: b.num,
                        success: true,
                        body: b.body,
                    }))
                }
                TmuxEvent::Error { num, .. } if num == bnum => {
                    let b = self.block.take().unwrap();
                    Some(ControlMessage::Reply(CommandReply {
                        num: b.num,
                        success: false,
                        body: b.body,
                    }))
                }
                _ => {
                    self.block.as_mut().unwrap().body.push(line.to_vec());
                    None
                }
            }
        } else {
            match parse_line(line) {
                TmuxEvent::Begin { num, .. } => {
                    self.block = Some(Block {
                        num,
                        body: Vec::new(),
                    });
                    None
                }
                ev => Some(ControlMessage::Notification(ev)),
            }
        }
    }
}

#[cfg(test)]
#[path = "notification_tests.rs"]
mod tests;
