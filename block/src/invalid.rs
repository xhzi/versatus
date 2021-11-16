use serde::{Serialize, Deserialize};
use std::fmt;
use std::error::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InvalidBlockErrorReason {
    NotTallestChain,
    BlockOutOfSequence,
    InvalidClaim,
    InvalidLastHash,
    InvalidStateHash,
    InvalidBlockHeight,
    InvalidBlockNonce,
    InvalidBlockReward,
    InvalidTxns,
    InvalidClaimPointers,
    General,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidBlockError {
    pub details: InvalidBlockErrorReason,
}

impl InvalidBlockErrorReason {
    pub fn to_str(&self) -> &str {
        match self {
            Self::BlockOutOfSequence => "block out of sequence",
            Self::General => "general invalid block",
            Self::InvalidBlockHeight => "invalid block height",
            Self::InvalidClaim => "invalid claim",
            Self::InvalidLastHash => "invalid last hash",
            Self::InvalidStateHash => "invalid state hash",
            Self::InvalidBlockNonce => "invalid block nonce",
            Self::InvalidBlockReward => "invalid block reward",
            Self::InvalidTxns => "invalid txns in block",
            Self::InvalidClaimPointers => "invalid claim pointers",
            Self::NotTallestChain => "blockchain proposed is shorter than my local chain"
        }
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        serde_json::to_string(self).unwrap().as_bytes().to_vec()
    }
}

impl fmt::Display for InvalidBlockError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}

impl Error for InvalidBlockError {
    fn description(&self) -> &str {
        &self.details.to_str()
    }
}

impl Error for InvalidBlockErrorReason {
    fn description(&self) -> &str {
        &self.to_str()
    }
}

impl fmt::Display for InvalidBlockErrorReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidBlockHeight => {
                write!(f, "invalid block height")
            }
            Self::InvalidClaim => {
                write!(f, "invalid claim")
            }
            Self::InvalidLastHash => {
                write!(f, "invalid last hash")
            }
            Self::InvalidStateHash => {
                write!(f, "invalid state hash")
            }
            Self::BlockOutOfSequence => {
                write!(f, "block out of sequence")
            }
            Self::InvalidBlockNonce => {
                write!(f, "invalid block nonce")
            }
            Self::InvalidBlockReward => {
                write!(f, "invalid block reward")
            }
            Self::InvalidTxns => {
                write!(f, "invalid txns in block")
            }
            Self::InvalidClaimPointers => {
                write!(f, "invalid claim pointers")
            }
            Self::General => {
                write!(f, "general invalid block error")
            }
            Self::NotTallestChain => {
                write!(f, "blockchain proposed shorter than local chain")
            }
        }
    }
}