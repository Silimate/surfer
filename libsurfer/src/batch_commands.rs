use camino::Utf8PathBuf;
use log::{error, info, trace};

use crate::{command_parser::get_parser, fzcmd::parse_command, message::Message, SystemState};

impl SystemState {
    /// After user messages are addressed, we try to execute batch commands as they are ready to run
    pub(crate) fn handle_batch_commands(&mut self) {
        // we only execute commands while we aren't waiting for background operations to complete
        while self.can_start_batch_command() {
            if let Some(cmd) = self.batch_messages.pop_front() {
                info!("Applying startup command: {cmd:?}");
                self.update(cmd);
            } else {
                break; // no more messages
            }
        }

        // if there are no messages and all operations have completed, we are done
        if !self.batch_messages_completed
            && self.batch_messages.is_empty()
            && self.can_start_batch_command()
        {
            self.batch_messages_completed = true;
        }
    }

    /// Returns whether it is OK to start a new batch command.
    pub(crate) fn can_start_batch_command(&self) -> bool {
        // if the progress tracker is none -> all operations have completed
        self.progress_tracker.is_none()
    }

    /// Returns true once all batch commands have been completed and their effects are all executed.
    pub fn batch_commands_completed(&self) -> bool {
        debug_assert!(
            self.batch_messages_completed || !self.batch_messages.is_empty(),
            "completed implies no commands"
        );
        self.batch_messages_completed
    }

    pub fn add_batch_commands<I: IntoIterator<Item = String>>(&mut self, commands: I) {
        let parsed = self.parse_batch_commands(commands);
        for msg in parsed {
            self.batch_messages.push_back(msg);
            self.batch_messages_completed = false;
        }
    }

    pub fn add_batch_messages<I: IntoIterator<Item = Message>>(&mut self, messages: I) {
        for msg in messages {
            self.batch_messages.push_back(msg);
            self.batch_messages_completed = false;
        }
    }

    pub fn add_batch_message(&mut self, msg: Message) {
        self.add_batch_messages([msg]);
    }

    fn parse_batch_commands<I: IntoIterator<Item = String>>(&mut self, cmds: I) -> Vec<Message> {
        trace!("Parsing batch commands");
        let parsed = cmds
            .into_iter()
            // Add line numbers
            .enumerate()
            // trace
            .map(|(no, line)| {
                trace!("{no: >2} {line}");
                (no, line)
            })
            // Make the line numbers start at 1 as is tradition
            .map(|(no, line)| (no + 1, line))
            .map(|(no, line)| (no, line.trim().to_string()))
            // NOTE: Safe unwrap. Split will always return one element
            .map(|(no, line)| (no, line.split('#').next().unwrap().to_string()))
            .filter(|(_no, line)| !line.is_empty())
            .flat_map(|(no, line)| {
                line.split(';')
                    .map(|cmd| (no, cmd.to_string()))
                    .collect::<Vec<_>>()
            })
            .filter_map(|(no, command)| {
                parse_command(&command, get_parser(self))
                    .map_err(|e| {
                        error!("Error on batch commands line {no}: {e:#?}");
                        e
                    })
                    .ok()
            })
            .collect::<Vec<_>>();

        parsed
    }
}

pub fn read_command_file(cmd_file: &Utf8PathBuf) -> Vec<String> {
    std::fs::read_to_string(cmd_file)
        .map_err(|e| error!("Failed to read commands from {cmd_file}. {e:#?}"))
        .ok()
        .map(|file_content| {
            file_content
                .lines()
                .map(std::string::ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}
