use std::{fs, io, process};
use std::io::Write;

use module::Module;
use pci::{PciBus, PciDevice};

static MODPROBE_NVIDIA: &'static [u8] = br#"# Automatically generated by system76-power
"#;

static MODPROBE_INTEL: &'static [u8] = br#"# Automatically generated by system76-power
blacklist bbswitch
blacklist nouveau
blacklist nvidia
blacklist nvidia-drm
blacklist nvidia-modeset
alias nouveau off
alias nvidia off
alias nvidia-drm off
alias nvidia-modeset off
"#;

pub struct Graphics {
    pub bus: PciBus,
    pub intel: Vec<PciDevice>,
    pub nvidia: Vec<PciDevice>,
    pub other: Vec<PciDevice>,
}

impl Graphics {
    pub fn new() -> io::Result<Graphics> {
        let bus = PciBus::new()?;

        eprintln!("Rescanning PCI bus");
        bus.rescan()?;

        let mut intel = Vec::new();
        let mut nvidia = Vec::new();
        let mut other = Vec::new();

        for dev in bus.devices()? {
            let class = dev.class()?;
            if class == 0x030000 {
                match dev.vendor()? {
                    0x10DE => {
                        eprintln!("{}: NVIDIA", dev.name());
                        nvidia.push(dev);
                    },
                    0x8086 => {
                        println!("{}: Intel", dev.name());
                        intel.push(dev);
                    },
                    vendor => {
                        println!("{}: Other({:X})", dev.name(), vendor);
                        other.push(dev);
                    },
                }
            }
        }

        Ok(Graphics {
            bus: bus,
            intel: intel,
            nvidia: nvidia,
            other: other,
        })
    }

    pub fn can_switch(&self) -> bool {
        self.intel.len() > 0 && self.nvidia.len() > 0
    }

    pub fn get_vendor(&self) -> io::Result<String> {
        if self.can_switch() {
            let modules = Module::all()?;

            if modules.iter().find(|module| module.name == "nouveau" || module.name == "nvidia").is_some() {
                Ok("nvidia".to_string())
            } else {
                Ok("intel".to_string())
            }
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "does not have switchable graphics"
            ))
        }
    }

    pub fn set_vendor(&self, vendor: &str) -> io::Result<()> {
        if self.can_switch() {
            {
                let path = "/etc/modprobe.d/system76-power.conf";
                eprintln!("Creating {}", path);
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(path)?;

                if vendor == "nvidia" {
                    file.write_all(MODPROBE_NVIDIA)?;
                } else {
                    file.write_all(MODPROBE_INTEL)?;
                }

                file.sync_all()?;
            }

            if vendor == "nvidia" {
                eprintln!("Enabling nvidia-fallback.service");
                let status = process::Command::new("systemctl").arg("enable").arg("nvidia-fallback.service").status()?;
                if ! status.success() {
                    // Error is ignored in case this service is removed
                    eprintln!("systemctl: failed with {}", status);
                }
            } else {
                eprintln!("Disabling nvidia-fallback.service");
                let status = process::Command::new("systemctl").arg("disable").arg("nvidia-fallback.service").status()?;
                if ! status.success() {
                    // Error is ignored in case this service is removed
                    eprintln!("systemctl: failed with {}", status);
                }
            }

            eprintln!("Updating initramfs");
            let status = process::Command::new("update-initramfs").arg("-u").status()?;
            if ! status.success() {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("update-initramfs: failed with {}", status)
                ));
            }

            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "does not have switchable graphics"
            ))
        }
    }

    pub fn get_power(&self) -> io::Result<bool> {
        if self.can_switch() {
            let mut power = false;
            for dev in self.nvidia.iter() {
                if dev.path().exists() {
                    power = true;
                }
            }
            Ok(power)
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "does not have switchable graphics"
            ))
        }
    }

    pub fn set_power(&self, power: bool) -> io::Result<()> {
        if self.can_switch() {
            if power {
                eprintln!("Enabling graphics power");
                self.bus.rescan()?;
            } else {
                eprintln!("Disabling graphics power");
                for dev in self.nvidia.iter() {
                    if dev.path().exists() {
                        match dev.driver() {
                            Ok(driver) => {
                                eprintln!("{}: in use by {}", dev.name(), driver.name());
                                return Err(io::Error::new(
                                    io::ErrorKind::Other,
                                    "dedicated graphics in use"
                                ));
                            },
                            Err(err) => match err.kind() {
                                io::ErrorKind::NotFound => {
                                    eprintln!("{}: Removing", dev.name());
                                    unsafe { dev.remove() }?;
                                },
                                _ => return Err(err),
                            }
                        }
                    } else {
                        eprintln!("{}: Already removed", dev.name());
                    }
                }
            }
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "does not have switchable graphics"
            ))
        }
    }

    pub fn auto_power(&self) -> io::Result<()> {
        if self.get_vendor()? == "nvidia" {
            self.set_power(true)
        } else {
            self.set_power(false)
        }
    }
}
