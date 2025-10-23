#![allow(dead_code)]

use nu_ansi_term::{Color, Style};
use std::fmt::{self, Display, Write};
use tracing::{
    Event, Level, Subscriber,
    field::{self, Field},
};
use tracing_subscriber::{
    field::VisitOutput,
    fmt::{
        FmtContext, FormatEvent, FormatFields, FormattedFields, format::Writer, time::FormatTime,
    },
    registry::LookupSpan,
};

pub struct CustomFormatter;
impl<S, N> FormatEvent<S, N> for CustomFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let f = &mut writer;

        let meta = event.metadata();

        let style = Style::new().dimmed();
        write!(f, "{}", style.prefix())?;

        tracing_subscriber::fmt::time().format_time(f)?;

        write!(f, "{} ", style.suffix())?;

        match *meta.level() {
            Level::TRACE => write!(f, "{} ", Color::Purple.paint("TRACE")),
            Level::DEBUG => write!(f, "{} ", Color::Blue.paint("DEBUG")),
            Level::INFO => write!(f, "{} ", Color::Green.paint(" INFO")),
            Level::WARN => write!(f, "{} ", Color::Yellow.paint(" WARN")),
            Level::ERROR => write!(f, "{} ", Color::Red.paint("ERROR")),
        }?;

        let target_style = style.bold();
        write!(
            f,
            "{}{}{}: ",
            target_style.prefix(),
            meta.target(),
            target_style.infix(style)
        )?;

        let mut v = PrettyVisitor::new(f.by_ref(), true).with_style(style);
        event.record(&mut v);
        v.finish()?;
        writeln!(f)?;

        let dimmed = Style::new().dimmed().italic();

        if let Some(file) = meta.file() {
            write!(f, "  {} {}", dimmed.paint("at"), file,)?;

            if let Some(line) = meta.line() {
                write!(f, ":{}", line)?;
            }
        }

        let thread = true;
        if thread {
            let thread = std::thread::current();
            if let Some(name) = thread.name() {
                write!(f, " {} ", dimmed.paint("on"))?;
                write!(f, "{}", name)?;
            }
            // write!(writer, " {:?}", thread.id())?;
        }
        f.write_char('\n')?;

        let bold = Style::new().bold();
        let span = event
            .parent()
            .and_then(|id| ctx.span(id))
            .or_else(|| ctx.lookup_current());

        let scope = span.into_iter().flat_map(|span| span.scope());

        for span in scope {
            let meta = span.metadata();
            write!(
                f,
                "  {} {}::{}",
                dimmed.paint("in"),
                meta.target(),
                bold.paint(meta.name()),
            )?;

            let ext = span.extensions();
            let fields = &ext
                .get::<FormattedFields<N>>()
                .expect("Unable to find FormattedFields in extensions; this is a bug");
            if !fields.is_empty() {
                write!(f, " {} {}", dimmed.paint("with"), fields)?;
            }
            f.write_char('\n')?;
        }

        f.write_char('\n')
    }
}

#[derive(Debug)]
pub struct PrettyVisitor<'a> {
    writer: tracing_subscriber::fmt::format::Writer<'a>,
    is_empty: bool,
    style: Style,
    result: std::fmt::Result,
}

impl<'a> PrettyVisitor<'a> {
    /// Returns a new default visitor that formats to the provided `writer`.
    ///
    /// # Arguments
    /// - `writer`: the writer to format to.
    /// - `is_empty`: whether or not any fields have been previously written to
    ///   that writer.
    fn new(writer: Writer<'a>, is_empty: bool) -> Self {
        Self {
            writer,
            is_empty,
            style: Style::default(),
            result: Ok(()),
        }
    }

    fn with_style(self, style: Style) -> Self {
        Self { style, ..self }
    }

    fn write_padded(&mut self, value: &impl std::fmt::Debug) {
        let padding = if self.is_empty {
            self.is_empty = false;
            "\n    "
        } else {
            ",\n    "
        };
        self.result = write!(self.writer, "{}{:?}", padding, value);
    }

    fn bold(&self) -> Style {
        if self.writer.has_ansi_escapes() {
            self.style.bold()
        } else {
            Style::new()
        }
    }
}

impl field::Visit for PrettyVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        if self.result.is_err() {
            return;
        }

        if field.name() == "message" {
            self.record_debug(field, &format_args!("{}", value))
        } else {
            self.record_debug(field, &value)
        }
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        if let Some(source) = value.source() {
            let bold = self.bold();
            self.record_debug(
                field,
                &format_args!(
                    "{}, {}{}.sources{}: {}",
                    Escape(&format_args!("{}", value)),
                    bold.prefix(),
                    field,
                    bold.infix(self.style),
                    ErrorSourceList(source),
                ),
            )
        } else {
            self.record_debug(field, &Escape(&format_args!("{}", value)))
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if self.result.is_err() {
            return;
        }
        let bold = self.bold();
        match field.name() {
            "message" => {
                self.result = write!(self.writer, "{}{:?}", self.style.prefix(), Escape(value));
            }
            // Skip fields that are actually log metadata that have already been handled
            name if name.starts_with("log.") => self.result = Ok(()),
            name if name.starts_with("r#") => self.write_padded(&format_args!(
                "{}{}{}: {:?}",
                bold.prefix(),
                &name[2..],
                bold.infix(self.style),
                value
            )),
            name => self.write_padded(&format_args!(
                "{}{}{}: {:?}",
                bold.prefix(),
                name,
                bold.infix(self.style),
                value
            )),
        };
    }
}

/// A wrapper that implements `fmt::Debug` and `fmt::Display` and escapes ANSI sequences on-the-fly.
/// This avoids creating intermediate strings while providing security against terminal injection.
struct Escape<T>(T);

/// Helper struct that escapes ANSI sequences as characters are written
struct EscapingWriter<'a, 'b> {
    inner: &'a mut fmt::Formatter<'b>,
}

impl<'a, 'b> Write for EscapingWriter<'a, 'b> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // Stream the string character by character, escaping ANSI and C1 control sequences
        for ch in s.chars() {
            match ch {
                // C0 control characters that can be used in terminal escape sequences
                '\x1b' => self.inner.write_str("\\x1b")?, // ESC
                '\x07' => self.inner.write_str("\\x07")?, // BEL
                '\x08' => self.inner.write_str("\\x08")?, // BS
                '\x0c' => self.inner.write_str("\\x0c")?, // FF
                '\x7f' => self.inner.write_str("\\x7f")?, // DEL

                // C1 control characters (\x80-\x9f) - 8-bit control codes
                // These can be used as alternative escape sequences in some terminals
                ch if ch as u32 >= 0x80 && ch as u32 <= 0x9f => {
                    write!(self.inner, "\\u{{{:x}}}", ch as u32)?
                }

                _ => self.inner.write_char(ch)?,
            }
        }
        Ok(())
    }
}

impl<T: fmt::Debug> fmt::Debug for Escape<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut escaping_writer = EscapingWriter { inner: f };
        write!(escaping_writer, "{:?}", self.0)
    }
}

impl<T: fmt::Display> fmt::Display for Escape<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut escaping_writer = EscapingWriter { inner: f };
        write!(escaping_writer, "{}", self.0)
    }
}

/// Renders an error into a list of sources, *including* the error
struct ErrorSourceList<'a>(&'a (dyn std::error::Error + 'static));

impl Display for ErrorSourceList<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_list();
        let mut curr = Some(self.0);
        while let Some(curr_err) = curr {
            list.entry(&Escape(&format_args!("{}", curr_err)));
            curr = curr_err.source();
        }
        list.finish()
    }
}
impl VisitOutput<fmt::Result> for PrettyVisitor<'_> {
    fn finish(mut self) -> fmt::Result {
        write!(&mut self.writer, "{}", self.style.suffix())?;
        self.result
    }
}
