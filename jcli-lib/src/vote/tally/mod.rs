mod decrypt_shares;
mod decryption_tally;

use super::Error;
#[cfg(feature = "structopt")]
use structopt::StructOpt;

#[cfg_attr(
    feature = "structopt",
    derive(StructOpt),
    structopt(rename_all = "kebab-case")
)]
pub enum Tally {
    /// Create a decryption share for private voting tally.
    ///
    /// The decryption share data will be printed in hexadecimal encoding
    /// on standard output.
    DecryptionShares(decryption_tally::TallyGenerateVotePlanDecryptionShares),
    /// Merge multiple sets of shares in a single object to be used in the
    /// decryption of a vote plan.
    MergeShares(decryption_tally::MergeShares),
    /// Decrypt all proposals in a vote plan.
    ///
    /// The decrypted tally data will be printed in hexadecimal encoding
    /// on standard output.
    DecryptResults(decrypt_shares::TallyVotePlanWithAllShares),
}

impl Tally {
    pub fn exec(self) -> Result<(), Error> {
        match self {
            Tally::DecryptionShares(cmd) => cmd.exec(),
            Tally::DecryptResults(cmd) => cmd.exec(),
            Tally::MergeShares(cmd) => cmd.exec(),
        }
    }
}
