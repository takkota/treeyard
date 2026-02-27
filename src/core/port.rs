use crate::core::compose_parser::PortMapping;

#[derive(Debug, Clone)]
pub struct PortAssignment {
    pub env_var: String,
    pub base_port: u16,
    pub computed_port: u16,
    pub is_shared: bool,
}

pub fn compute_ports(
    mappings: &[PortMapping],
    slot: u32,
    step: u16,
    shared_services: &[String],
) -> Result<Vec<PortAssignment>, crate::error::Error> {
    let mut assignments = Vec::new();

    for m in mappings {
        let is_shared = shared_services.iter().any(|s| s == &m.service);
        let computed = if is_shared {
            m.default_port as u32
        } else {
            m.default_port as u32 + slot * step as u32
        };

        if computed > 65535 {
            return Err(crate::error::Error::PortOverflow {
                env_var: m.env_var.clone(),
                base: m.default_port,
                computed,
                slot,
                step,
            });
        }

        assignments.push(PortAssignment {
            env_var: m.env_var.clone(),
            base_port: m.default_port,
            computed_port: computed as u16,
            is_shared,
        });
    }

    Ok(assignments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::compose_parser::PortMapping;

    #[test]
    fn test_compute_slot_0() {
        let mappings = vec![PortMapping {
            env_var: "WEB_PORT".into(),
            default_port: 3000,
            container_port: 3000,
            service: "web".into(),
        }];
        let result = compute_ports(&mappings, 0, 10, &[]).unwrap();
        assert_eq!(result[0].computed_port, 3000);
    }

    #[test]
    fn test_compute_slot_1() {
        let mappings = vec![PortMapping {
            env_var: "WEB_PORT".into(),
            default_port: 3000,
            container_port: 3000,
            service: "web".into(),
        }];
        let result = compute_ports(&mappings, 1, 10, &[]).unwrap();
        assert_eq!(result[0].computed_port, 3010);
    }

    #[test]
    fn test_shared_service_no_offset() {
        let mappings = vec![PortMapping {
            env_var: "DB_PORT".into(),
            default_port: 5432,
            container_port: 5432,
            service: "postgres".into(),
        }];
        let shared = vec!["postgres".to_string()];
        let result = compute_ports(&mappings, 2, 10, &shared).unwrap();
        assert_eq!(result[0].computed_port, 5432);
        assert!(result[0].is_shared);
    }

    #[test]
    fn test_port_overflow() {
        let mappings = vec![PortMapping {
            env_var: "PORT".into(),
            default_port: 65000,
            container_port: 65000,
            service: "web".into(),
        }];
        let result = compute_ports(&mappings, 10, 100, &[]);
        assert!(result.is_err());
    }
}
