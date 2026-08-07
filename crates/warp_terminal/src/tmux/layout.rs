//! tmux layout-string parse + emit + checksum.
//!
//! Grammar: `"<checksum>,<cell>"` where `checksum` is 4 hex digits and a
//! cell is `"<W>x<H>,<X>,<Y>"` optionally followed by `,<paneid>` (leaf),
//! `{<cell>,...}` (horizontal / left-right split) or `[<cell>,...]`
//! (vertical / top-bottom split).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDir {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutNode {
    Leaf {
        pane: u32,
        w: u32,
        h: u32,
        x: u32,
        y: u32,
    },
    Split {
        dir: SplitDir,
        w: u32,
        h: u32,
        x: u32,
        y: u32,
        children: Vec<LayoutNode>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub checksum: u16,
    pub root: LayoutNode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    /// Missing the `<checksum>,` prefix or it is not 4 hex digits.
    BadChecksum,
    /// Parsed checksum does not match the computed checksum of the body.
    ChecksumMismatch { expected: u16, actual: u16 },
    /// Malformed cell / dimensions / structure.
    Malformed,
    /// Extra unparsed input remained after the root cell.
    TrailingInput,
}

/// tmux layout checksum over the body bytes (everything after `<checksum>,`).
pub fn checksum(body: &str) -> u16 {
    let mut c: u16 = 0;
    for b in body.bytes() {
        c = (c >> 1) + ((c & 1) << 15);
        c = c.wrapping_add(b as u16);
    }
    c
}

pub fn emit_body(root: &LayoutNode) -> String {
    let mut out = String::new();
    emit_node(root, &mut out);
    out
}

pub fn emit_layout(root: &LayoutNode) -> String {
    let body = emit_body(root);
    let c = checksum(&body);
    format!("{c:04x},{body}")
}

fn emit_node(node: &LayoutNode, out: &mut String) {
    match node {
        LayoutNode::Leaf { pane, w, h, x, y } => {
            out.push_str(&format!("{w}x{h},{x},{y},{pane}"));
        }
        LayoutNode::Split {
            dir,
            w,
            h,
            x,
            y,
            children,
        } => {
            out.push_str(&format!("{w}x{h},{x},{y}"));
            let (open, close) = match dir {
                SplitDir::Horizontal => ('{', '}'),
                SplitDir::Vertical => ('[', ']'),
            };
            out.push(open);
            for (i, child) in children.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                emit_node(child, out);
            }
            out.push(close);
        }
    }
}

pub fn parse_layout(s: &str) -> Result<Window, LayoutError> {
    // Split off the 4-hex-digit checksum prefix: "xxxx,<body>".
    let comma = s.find(',').ok_or(LayoutError::BadChecksum)?;
    let (hex, rest) = s.split_at(comma);
    if hex.len() != 4 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(LayoutError::BadChecksum);
    }
    let expected = u16::from_str_radix(hex, 16).map_err(|_| LayoutError::BadChecksum)?;
    let body = &rest[1..]; // skip the comma

    let actual = checksum(body);
    if actual != expected {
        return Err(LayoutError::ChecksumMismatch { expected, actual });
    }

    let mut p = Parser {
        b: body.as_bytes(),
        i: 0,
    };
    let root = p.parse_cell()?;
    if p.i != p.b.len() {
        return Err(LayoutError::TrailingInput);
    }
    Ok(Window {
        checksum: expected,
        root,
    })
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    /// Parse a decimal u32; requires at least one digit; guards overflow.
    fn parse_u32(&mut self) -> Result<u32, LayoutError> {
        let start = self.i;
        let mut val: u32 = 0;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                let d = (c - b'0') as u32;
                val = val
                    .checked_mul(10)
                    .and_then(|v| v.checked_add(d))
                    .ok_or(LayoutError::Malformed)?;
                self.i += 1;
            } else {
                break;
            }
        }
        if self.i == start {
            return Err(LayoutError::Malformed);
        }
        Ok(val)
    }

    fn expect(&mut self, c: u8) -> Result<(), LayoutError> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(LayoutError::Malformed)
        }
    }

    fn parse_cell(&mut self) -> Result<LayoutNode, LayoutError> {
        let w = self.parse_u32()?;
        self.expect(b'x')?;
        let h = self.parse_u32()?;
        self.expect(b',')?;
        let x = self.parse_u32()?;
        self.expect(b',')?;
        let y = self.parse_u32()?;

        match self.peek() {
            Some(b'{') => {
                let children = self.parse_children(b'{', b'}')?;
                Ok(LayoutNode::Split {
                    dir: SplitDir::Horizontal,
                    w,
                    h,
                    x,
                    y,
                    children,
                })
            }
            Some(b'[') => {
                let children = self.parse_children(b'[', b']')?;
                Ok(LayoutNode::Split {
                    dir: SplitDir::Vertical,
                    w,
                    h,
                    x,
                    y,
                    children,
                })
            }
            Some(b',') => {
                self.i += 1;
                let pane = self.parse_u32()?;
                Ok(LayoutNode::Leaf { pane, w, h, x, y })
            }
            _ => Err(LayoutError::Malformed),
        }
    }

    fn parse_children(&mut self, open: u8, close: u8) -> Result<Vec<LayoutNode>, LayoutError> {
        self.expect(open)?;
        let mut children = Vec::new();
        loop {
            children.push(self.parse_cell()?);
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                }
                Some(c) if c == close => {
                    self.i += 1;
                    break;
                }
                _ => return Err(LayoutError::Malformed),
            }
        }
        Ok(children)
    }
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
