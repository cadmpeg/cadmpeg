// SPDX-License-Identifier: Apache-2.0
//! Parse-time rejection of JSON selectors on commands without a JSON mode.

use clap::{Arg, ArgMatches, Args, Command, FromArgMatches};

/// Adds the pinned teaching error without retaining a rejected argument.
#[derive(Debug)]
pub(crate) struct RejectJson;

impl Args for RejectJson {
    fn augment_args(command: Command) -> Command {
        let message = match command.get_name() {
            "convert" => {
                "--json is not an output selector on convert; the artifact format comes from \
         --format/-o, and the machine-readable report from --report FILE, projected \
         with `cadmpeg query`"
            }
            "dump" => {
                "dump writes the CADIR JSON artifact itself; its standard output is \
         already JSON when -o is omitted; the dump report goes to --report FILE, \
         projected with `cadmpeg query`"
            }
            _ => {
                "this inspect tool has no JSON form; JSON lives on `inspect FILE --json` \
         (the container summary), `inspect container --json`, and `inspect \
         find --json`"
            }
        };
        command.arg(
            Arg::new("json")
                .long("json")
                .hide(true)
                .num_args(0)
                .default_missing_value("true")
                .value_parser(move |_: &str| -> Result<String, String> { Err(message.to_owned()) }),
        )
    }

    fn augment_args_for_update(command: Command) -> Command {
        Self::augment_args(command)
    }
}

impl FromArgMatches for RejectJson {
    fn from_arg_matches(_: &ArgMatches) -> Result<Self, clap::Error> {
        Ok(Self)
    }

    fn update_from_arg_matches(&mut self, _: &ArgMatches) -> Result<(), clap::Error> {
        Ok(())
    }
}
