use crate::Console;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityStatus {
    Planned,
    SoftwareModel,
    AcceptancePrototype,
    HardwareActive,
    Verified,
}

impl CapabilityStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::SoftwareModel => "software_model",
            Self::AcceptancePrototype => "acceptance_prototype",
            Self::HardwareActive => "hardware_active",
            Self::Verified => "verified",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capability {
    pub id: &'static str,
    pub name: &'static str,
    pub status: CapabilityStatus,
    pub milestone: &'static str,
    pub boot_label: &'static str,
}

pub fn print_registry(console: &mut dyn Console) {
    console.write_line("Capability truth registry:");
    for capability in crate::generated::capabilities::CAPABILITIES {
        console.write_str("[CAP] ");
        console.write_str(capability.id);
        console.write_str(" ");
        console.write_str(capability.status.as_str());
        console.write_str(" - ");
        console.write_line(capability.name);
    }
}
