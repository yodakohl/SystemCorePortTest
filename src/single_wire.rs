#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SingleWireFlowDirection {
    SchedulerToTarget = 0x0B,
    TargetToScheduler = 0x0C,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SingleWireSchedulerContent {
    Command = 0x1A,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SingleWireTargetContent {
    None = 0x15,
    Process = 0x11,
    Log = 0x12,
    Command = 0x13,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SingleWireFrameContent {
    Unsolicited = 0x05,
    Cut = 0x0F,
    End = 0x17,
}
