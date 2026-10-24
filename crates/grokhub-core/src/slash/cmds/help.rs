use crate::slash::registry::register_slash;
use crate::slash::Slash;

register_slash! {
    /// `/help`: the command list, as chat text.
    HELP {
        names: ["/help"],
        effect: Local,
        sample: "/help",
        parse: |_rest| Some(Slash::Help),
    }
}
