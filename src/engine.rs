use std::{
    io::{self, BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

/// An lc0 process speaking UCI, loaded with one Maia weights file.
pub struct Engine {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Engine {
    pub fn start(bin: &str, weights: &Path) -> io::Result<Engine> {
        let mut child = Command::new(bin)
            .arg(format!("--weights={}", weights.display()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut engine = Engine {
            child,
            stdin,
            stdout,
        };
        engine.send("uci")?;
        engine.wait_for("uciok")?;
        engine.send("isready")?;
        engine.wait_for("readyok")?;
        Ok(engine)
    }

    fn send(&mut self, cmd: &str) -> io::Result<()> {
        writeln!(self.stdin, "{cmd}")?;
        self.stdin.flush()
    }

    fn wait_for(&mut self, prefix: &str) -> io::Result<String> {
        let mut line = String::new();
        loop {
            line.clear();
            if self.stdout.read_line(&mut line)? == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "lc0 exited"));
            }
            if line.starts_with(prefix) {
                return Ok(line.trim().to_string());
            }
        }
    }

    /// Best move in UCI notation for the position reached by `moves` from `start`
    /// (a FEN, or the standard position when `None`).
    pub fn best_move(
        &mut self,
        start: Option<&str>,
        moves: &[String],
        nodes: u32,
    ) -> io::Result<String> {
        let base = match start {
            Some(fen) => format!("position fen {fen}"),
            None => "position startpos".to_string(),
        };
        if moves.is_empty() {
            self.send(&base)?;
        } else {
            self.send(&format!("{base} moves {}", moves.join(" ")))?;
        }
        self.send(&format!("go nodes {nodes}"))?;
        let line = self.wait_for("bestmove")?;
        Ok(line.split_whitespace().nth(1).unwrap_or("").to_string())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.send("quit");
        let _ = self.child.wait();
    }
}
