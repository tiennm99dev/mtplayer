//! The interactive license prompt. It always prints the whole text and has no way to skip:
//! no `--yes`, no environment variable, no configuration key.

use std::io::{self, BufRead, IsTerminal, Write};

use mtp_sdk::hash::Sha1Hex;
use mtp_sdk::{Authority, Decision, Error, GPL_3, LicensePrompt, LicenseText, Result, Via};

/// Asks on the terminal. It refuses to run when stdin or stdout isn't a terminal.
pub struct CliPrompt {
    authority: Option<Authority>,
}

impl CliPrompt {
    pub fn new() -> Self {
        Self { authority: None }
    }

    fn is_interactive() -> bool {
        io::stdin().is_terminal() && io::stdout().is_terminal()
    }
}

fn ask(question: &str) -> Result<String> {
    print!("{question} ");
    let failed = |source: io::Error| Error::Io {
        path: "stdin".into(),
        source,
    };
    io::stdout().flush().map_err(failed)?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer).map_err(failed)?;
    Ok(answer.trim().to_string())
}

/// Reads a `[y/N]` answer: an empty one takes the default, and anything unrecognized is `None`,
/// because a mistyped answer must not record an authority the user didn't state.
fn yes_no(answer: &str) -> Option<bool> {
    match answer.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Some(true),
        "" | "n" | "no" => Some(false),
        _ => None,
    }
}

/// The authority an acknowledgement records: what the user said in this run, and never
/// something they weren't asked.
fn acknowledgement_authority(stated: Option<Authority>) -> Authority {
    stated.unwrap_or(Authority::NotStated)
}

impl LicensePrompt for CliPrompt {
    fn available(&self, _id: &str) -> bool {
        Self::is_interactive()
    }

    fn decide(&mut self, text: &LicenseText, reviewed: Option<&Sha1Hex>) -> Result<Decision> {
        if !Self::is_interactive() {
            return Err(Error::LicenseNotAccepted {
                id: text.id().to_string(),
            });
        }
        println!();
        println!("license  {}", text.id());
        println!("source   {}", text.source());
        println!("sha1     {}", text.sha1());
        if reviewed.is_some_and(|hash| hash != text.sha1()) {
            println!("note     this text differs from the one reviewed for this MTPlayer release");
        }
        println!();
        println!("{}", text.text());
        println!();

        // The GPL doesn't require acceptance to run a program, so it is acknowledged, and
        // the employer question belongs to Google's license alone.
        if text.id() == GPL_3 {
            let answer = ask(
                "Magisk's binary is downloaded under the license shown above. Type \"I acknowledge\" to continue:",
            )?;
            if answer != "I acknowledge" {
                return Ok(Decision::Decline);
            }
            return Ok(Decision::Accept {
                authority: acknowledgement_authority(self.authority),
                via: Via::Prompt,
            });
        }

        let answer = ask("Type \"I accept\" to accept the Android SDK License shown above:")?;
        if answer != "I accept" {
            return Ok(Decision::Decline);
        }
        let authority = match yes_no(&ask("Are you installing for an employer? [y/N]")?) {
            Some(false) => Authority::Personal,
            Some(true) => {
                let confirmed = ask(
                    "Confirm you have full legal authority to bind your employer to this license (section 2.4) [y/N]",
                )?;
                if yes_no(&confirmed) != Some(true) {
                    return Ok(Decision::Decline);
                }
                Authority::Employer
            }
            None => return Ok(Decision::Decline),
        };
        self.authority = Some(authority);
        Ok(Decision::Accept {
            authority,
            via: Via::Prompt,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_clear_answers_count_as_yes_or_no() {
        for answer in ["y", "Y", "yes", "YES", "  yes "] {
            assert_eq!(yes_no(answer), Some(true), "{answer:?}");
        }
        for answer in ["", "   ", "n", "N", "no", "NO"] {
            assert_eq!(yes_no(answer), Some(false), "{answer:?}");
        }
        for answer in ["yep", "ye", "yse", "oui", "nope", "x"] {
            assert_eq!(yes_no(answer), None, "{answer:?}");
        }
    }

    #[test]
    fn an_acknowledgement_records_only_what_the_user_stated() {
        assert_eq!(acknowledgement_authority(None), Authority::NotStated);
        assert_eq!(
            acknowledgement_authority(Some(Authority::Employer)),
            Authority::Employer
        );
    }
}
