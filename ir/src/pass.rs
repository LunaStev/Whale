// SPDX-License-Identifier: MPL-2.0
//! Verified, transactional pass boundaries. No optimization pipeline is enabled.
use crate::{IrLimits, Module, VerifyError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalysisKind {
    Cfg,
    Values,
    Effects,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Invalidates {
    pub cfg: bool,
    pub values: bool,
    pub effects: bool,
}

impl Invalidates {
    pub const ALL: Self = Self { cfg: true, values: true, effects: true };
}

#[derive(Debug, Default)]
pub struct AnalysisState {
    identity: std::sync::Arc<()>,
    epochs: [u64; 3],
}

impl AnalysisState {
    pub fn epoch(&self, kind: AnalysisKind) -> u64 {
        self.epochs[kind as usize]
    }

    fn invalidate(&mut self, which: Invalidates) -> Result<(), String> {
        let mut next = self.epochs;
        for (i, invalid) in [which.cfg, which.values, which.effects].into_iter().enumerate() {
            if invalid {
                next[i] = next[i].checked_add(1).ok_or("analysis epoch exhausted")?;
            }
        }
        self.epochs = next;
        Ok(())
    }
}

/// A cache belongs to one module/AnalysisState pair. Direct edits require explicit
/// invalidation by the caller; the runner invalidates every successful mutation.
pub struct AnalysisCache<T> {
    identity: std::sync::Arc<()>,
    kind: AnalysisKind,
    epoch: u64,
    value: T,
}

impl<T> AnalysisCache<T> {
    pub fn new(kind: AnalysisKind, state: &AnalysisState, value: T) -> Self {
        Self { identity: state.identity.clone(), kind, epoch: state.epoch(kind), value }
    }

    pub fn get(&self, state: &AnalysisState) -> Option<&T> {
        (std::sync::Arc::ptr_eq(&self.identity, &state.identity)
            && self.epoch == state.epoch(self.kind))
        .then_some(&self.value)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PassChange {
    pub changed: bool,
    pub invalidates: Invalidates,
}

pub trait Pass {
    fn name(&self) -> &str;

    fn run(&mut self, module: &mut Module) -> Result<PassChange, String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassStage {
    Before,
    Transform,
    After,
    Contract,
}

#[derive(Debug)]
pub enum PassFailure {
    Verification(Box<VerifyError>),
    Message(String),
}

#[derive(Debug)]
pub struct PassError {
    pub pass: String,
    pub target: String,
    pub stage: PassStage,
    pub cause: PassFailure,
}

impl std::fmt::Display for PassError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pass {:?} on target {:?} at {:?}: ", self.pass, self.target, self.stage)?;
        match &self.cause {
            PassFailure::Verification(e) => write!(f, "{e:?}"),
            PassFailure::Message(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for PassError {}

pub fn run_pass(
    module: &mut Module,
    pass: &mut impl Pass,
    analyses: &mut AnalysisState,
) -> Result<PassChange, PassError> {
    run_pass_with_limits(module, pass, analyses, IrLimits::default())
}

/// Always verify before and after. On transform, verification or contract failure
/// restore the complete original module, including diagnostic source identities.
/// All successful mutations invalidate CFG/value/effect analyses conservatively.
pub fn run_pass_with_limits(
    module: &mut Module,
    pass: &mut impl Pass,
    analyses: &mut AnalysisState,
    limits: IrLimits,
) -> Result<PassChange, PassError> {
    let name = pass.name().to_owned();
    let target = module.target.clone();
    let error =
        |stage, cause| PassError { pass: name.clone(), target: target.clone(), stage, cause };
    crate::verify_module_with_limits(module, limits)
        .map_err(|e| error(PassStage::Before, PassFailure::Verification(Box::new(e))))?;
    let before = module.clone();
    let result = (|| {
        let report =
            pass.run(module).map_err(|e| error(PassStage::Transform, PassFailure::Message(e)))?;
        crate::verify_module_with_limits(module, limits)
            .map_err(|e| error(PassStage::After, PassFailure::Verification(Box::new(e))))?;
        let changed = *module != before;
        if report.changed != changed
            || changed && report.invalidates != Invalidates::ALL
            || !changed && report.invalidates != Invalidates::default()
        {
            return Err(error(PassStage::Contract,PassFailure::Message("report must match the mutation and explicitly invalidate all analyses on change".into())));
        }
        analyses
            .invalidate(report.invalidates)
            .map_err(|e| error(PassStage::Contract, PassFailure::Message(e)))?;
        Ok(report)
    })();
    if result.is_err() {
        *module = before;
    }
    result
}
