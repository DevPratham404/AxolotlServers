//! Pure translation from a [`CreateServerSpec`] to a Docker container definition.
//!
//! Kept free of I/O so the port-exposure and resource-limit rules can be unit
//! tested without a Docker daemon.

use std::collections::HashMap;

use bollard::models::{
    ContainerCreateBody, ContainerUpdateBody, HostConfig, PortBinding, RestartPolicy,
    RestartPolicyNameEnum,
};
use cluster_shared::CreateServerSpec;

/// Minecraft's in-container Java game port.
pub const CONTAINER_PORT: u16 = 25565;
/// BungeeCord and Waterfall listen on this port inside `itzg/mc-proxy`.
pub const PROXY_CONTAINER_PORT: u16 = 25577;
/// Bedrock Edition's in-container game port.
pub const BEDROCK_CONTAINER_PORT: u16 = 19132;

/// RCON listens here inside the container. It is published on the host loopback
/// only (never the public interface) because the daemon reaches it over TCP.
pub const RCON_PORT: u16 = 25575;

/// Container-side game port for the runtime.
///
/// BungeeCord/Waterfall default to 25577 in the mc-proxy image, while Velocity
/// keeps Velocity's own default of 25565 unless its `velocity.toml` is edited.
/// Publishing 25577 for a Velocity container would leave the host port bound to
/// a listener that does not exist.
pub fn game_container_port(server_type: cluster_shared::ServerType) -> u16 {
    if matches!(
        server_type,
        cluster_shared::ServerType::Bungeecord | cluster_shared::ServerType::Waterfall
    ) {
        PROXY_CONTAINER_PORT
    } else if matches!(server_type, cluster_shared::ServerType::Bedrock) {
        BEDROCK_CONTAINER_PORT
    } else {
        CONTAINER_PORT
    }
}

fn game_port_key(spec: &CreateServerSpec) -> String {
    let port = game_container_port(spec.server_type);
    let protocol = if matches!(spec.server_type, cluster_shared::ServerType::Bedrock) {
        "udp"
    } else {
        "tcp"
    };
    format!("{port}/{protocol}")
}

/// Deterministic identity: the daemon never relies on a stored container id.
pub fn container_name(server_id: &str) -> String {
    format!("mc-{server_id}")
}

/// Deterministic host port for a server's RCON endpoint.
///
/// The mapping lives in `30000..=39999`, which keeps RCON out of the usual
/// Minecraft port range and works on Docker Desktop (where container IPs are
/// not routable from the host). Collisions are possible in theory for very large
/// deployments; a stored port column can replace this in a later milestone.
pub fn rcon_host_port(server_id: &str) -> u16 {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(server_id.as_bytes());
    let value = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    30_000 + (value % 10_000) as u16
}

/// Deterministic per-server RCON password derived from the node secret.
///
/// Keeping it out of the database means the daemon can recompute it for any
/// server it owns; rotating `DAEMON_SHARED_SECRET` invalidates old values and
/// requires containers to be recreated (the env is baked in at create time).
pub fn rcon_password(secret: &str, server_id: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(b":rcon:");
    hasher.update(server_id.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn memory_bytes(ram_mb: i64) -> i64 {
    ram_mb.saturating_mul(1024 * 1024)
}

/// itzg accepts `2G`, `2048M`, …; keep the value human-readable.
pub fn memory_env(ram_mb: i64) -> String {
    if ram_mb > 0 && ram_mb % 1024 == 0 {
        format!("{}G", ram_mb / 1024)
    } else {
        format!("{ram_mb}M")
    }
}

pub fn nano_cpus(cpu_limit: f64) -> i64 {
    (cpu_limit * 1_000_000_000.0).round() as i64
}

/// Environment for the itzg image — a deliberately small allow-list.
pub fn env_for(spec: &CreateServerSpec, rcon_password: &str) -> Vec<String> {
    let mut env = vec![
        "EULA=TRUE".to_string(),
        format!("TYPE={}", spec.server_type.itzg_type()),
        format!("MEMORY={}", memory_env(spec.ram_mb)),
        format!("SERVER_NAME={}", spec.name),
        // The config editor owns server.properties; env must not overwrite it.
        "OVERRIDE_SERVER_PROPERTIES=false".to_string(),
        // itzg resolves this to a concrete Minecraft version (or the latest
        // release when set to LATEST).
        format!("VERSION={}", spec.game_version),
        format!("LEVEL={}", spec.world_name),
        format!("LEVEL_TYPE={}", spec.world_type),
        format!("DIFFICULTY={}", spec.difficulty),
        format!("MODE={}", spec.gamemode),
        // RCON is the control plane's only in-game command channel for v1.
        "ENABLE_RCON=TRUE".to_string(),
        format!("RCON_PORT={RCON_PORT}"),
        format!("RCON_PASSWORD={rcon_password}"),
    ];
    if !spec.world_seed.is_empty() {
        env.push(format!("SEED={}", spec.world_seed));
    }

    if matches!(spec.server_type, cluster_shared::ServerType::Arclight) {
        // itzg defaults to NeoForge, but pinning it makes the downloaded jar and
        // the mod loader explicit and prevents a future default change from
        // silently switching the runtime family.
        env.push("ARCLIGHT_TYPE=NEOFORGE".to_string());
    }

    if spec.is_behind_proxy {
        // The proxy performs authentication; backends must accept its players.
        env.push("ONLINE_MODE=false".to_string());
    }

    env
}

pub fn host_config(spec: &CreateServerSpec, network: &str, data_root: &str) -> HostConfig {
    let memory = memory_bytes(spec.ram_mb);

    // Proxy-hidden backends are reachable only on the Docker network: expose the
    // port inside the container but bind nothing on the host. Everyone else gets
    // a strict host_port -> 25565 mapping. RCON is always bound to loopback so
    // the daemon can reach it on Docker Desktop too.
    let mut bindings = HashMap::new();
    if !spec.is_behind_proxy {
        bindings.insert(
            game_port_key(spec),
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(spec.game_port.to_string()),
            }]),
        );
    }
    bindings.insert(
        format!("{RCON_PORT}/tcp"),
        Some(vec![PortBinding {
            host_ip: Some("127.0.0.1".to_string()),
            host_port: Some(rcon_host_port(&spec.id).to_string()),
        }]),
    );

    HostConfig {
        // Hard limits. memory_swap == memory disables swap growth.
        memory: Some(memory),
        memory_swap: Some(memory),
        nano_cpus: Some(nano_cpus(spec.cpu_limit)),
        pids_limit: Some(512),
        port_bindings: Some(bindings),
        network_mode: Some(network.to_string()),
        binds: Some(vec![format!(
            "{}/{}/data:{}",
            data_root.trim_end_matches('/'),
            spec.id,
            if spec.server_type.is_proxy() {
                "/server"
            } else {
                "/data"
            }
        )]),
        restart_policy: Some(RestartPolicy {
            name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
            maximum_retry_count: None,
        }),
        security_opt: Some(vec!["no-new-privileges".to_string()]),
        ..Default::default()
    }
}

/// The HostConfig subset that Docker can change on an existing container.
///
/// Applied before every start so edits made after provisioning (scaling) reach
/// the container even though its HostConfig was fixed at creation time.
pub fn resource_update(ram_mb: i64, cpu_limit: f64) -> ContainerUpdateBody {
    let memory = memory_bytes(ram_mb);
    ContainerUpdateBody {
        memory: Some(memory),
        memory_swap: Some(memory),
        nano_cpus: Some(nano_cpus(cpu_limit)),
        pids_limit: Some(512),
        ..Default::default()
    }
}

pub fn create_body(
    spec: &CreateServerSpec,
    image: &str,
    network: &str,
    data_root: &str,
    rcon_password: &str,
) -> ContainerCreateBody {
    let labels = HashMap::from([
        ("console.managed".to_string(), "true".to_string()),
        ("console.server".to_string(), spec.id.clone()),
        (
            "console.type".to_string(),
            spec.server_type.as_str().to_string(),
        ),
    ]);

    ContainerCreateBody {
        image: Some(image.to_string()),
        env: Some(env_for(spec, rcon_password)),
        exposed_ports: Some(vec![game_port_key(spec), format!("{RCON_PORT}/tcp")]),
        labels: Some(labels),
        host_config: Some(host_config(spec, network, data_root)),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cluster_shared::ServerType;

    fn spec(is_behind_proxy: bool) -> CreateServerSpec {
        CreateServerSpec {
            id: "01a1d37d-4700-7107-a4e8-72e044c3a464".into(),
            name: "survival".into(),
            server_type: ServerType::Paper,
            game_version: "1.21.1".into(),
            game_port: 25565,
            ram_mb: 4096,
            cpu_limit: 2.0,
            is_behind_proxy,
            world_name: "world".into(),
            world_seed: String::new(),
            world_type: "default".into(),
            difficulty: "normal".into(),
            gamemode: "survival".into(),
        }
    }

    #[test]
    fn publishes_host_port_for_standalone_servers() {
        let config = host_config(&spec(false), "mc-net", "/srv/mc");
        let bindings = config.port_bindings.expect("port bindings");
        let binding = bindings.get("25565/tcp").expect("25565/tcp binding");
        let binding = binding.as_ref().expect("binding list");
        assert_eq!(binding[0].host_port.as_deref(), Some("25565"));
        assert_eq!(binding[0].host_ip.as_deref(), Some("0.0.0.0"));
    }

    #[test]
    fn hides_host_port_for_proxy_backends() {
        let config = host_config(&spec(true), "mc-net", "/srv/mc");
        let bindings = config.port_bindings.expect("rcon binding remains");
        assert!(
            !bindings.contains_key("25565/tcp"),
            "backends must not publish a host game port"
        );
        assert!(
            bindings.contains_key("25575/tcp"),
            "rcon stays reachable on the loopback"
        );

        // The port is still exposed inside the container/network.
        let body = create_body(
            &spec(true),
            "itzg/minecraft-server:latest",
            "mc-net",
            "/srv/mc",
            "secret",
        );
        assert_eq!(
            body.exposed_ports,
            Some(vec!["25565/tcp".to_string(), "25575/tcp".to_string()])
        );
        assert_eq!(
            body.host_config
                .expect("host config")
                .network_mode
                .as_deref(),
            Some("mc-net")
        );
    }

    #[test]
    fn publishes_proxy_and_bedrock_container_ports() {
        let mut proxy = spec(false);
        proxy.server_type = ServerType::Waterfall;
        proxy.game_port = 25565;
        let bindings = host_config(&proxy, "mc-net", "/srv/mc")
            .port_bindings
            .expect("proxy bindings");
        let binding = bindings.get("25577/tcp").expect("proxy listener");
        assert_eq!(
            binding.as_ref().expect("binding list")[0]
                .host_port
                .as_deref(),
            Some("25565")
        );
        assert!(!bindings.contains_key("25565/tcp"));

        // Velocity keeps its own 25565 default; publishing 25577 would point at
        // a closed port.
        let mut velocity = spec(false);
        velocity.server_type = ServerType::Velocity;
        velocity.game_port = 25566;
        let bindings = host_config(&velocity, "mc-net", "/srv/mc")
            .port_bindings
            .expect("velocity bindings");
        let binding = bindings.get("25565/tcp").expect("velocity listener");
        assert_eq!(
            binding.as_ref().expect("binding list")[0]
                .host_port
                .as_deref(),
            Some("25566")
        );
        assert!(!bindings.contains_key("25577/tcp"));

        let mut bedrock = spec(false);
        bedrock.server_type = ServerType::Bedrock;
        let bindings = host_config(&bedrock, "mc-net", "/srv/mc")
            .port_bindings
            .expect("bedrock bindings");
        assert!(bindings.contains_key("19132/udp"));
        assert!(!bindings.contains_key("25565/tcp"));
    }

    #[test]
    fn adds_arclight_loader_and_avoids_invalid_types() {
        let mut arclight = spec(false);
        arclight.server_type = ServerType::Arclight;
        let env = env_for(&arclight, "secret");
        assert!(env.contains(&"TYPE=ARCLIGHT".to_string()));
        assert!(env.contains(&"ARCLIGHT_TYPE=NEOFORGE".to_string()));

        assert_eq!(ServerType::CatServer.itzg_type(), "CUSTOM");
    }

    #[test]
    fn enforces_strict_resource_limits() {
        let config = host_config(&spec(false), "mc-net", "/srv/mc");
        let expected = 4096 * 1024 * 1024;
        assert_eq!(config.memory, Some(expected));
        assert_eq!(
            config.memory_swap,
            Some(expected),
            "swap must not exceed the RAM limit"
        );
        assert_eq!(config.nano_cpus, Some(2_000_000_000));
        assert_eq!(config.pids_limit, Some(512));
        assert_eq!(
            config.security_opt,
            Some(vec!["no-new-privileges".to_string()])
        );
    }

    #[test]
    fn builds_resource_updates_for_scaling() {
        let update = resource_update(6144, 3.5);
        assert_eq!(update.memory, Some(6144 * 1024 * 1024));
        assert_eq!(update.memory_swap, Some(6144 * 1024 * 1024));
        assert_eq!(update.nano_cpus, Some(3_500_000_000));
        assert_eq!(update.pids_limit, Some(512));
    }

    #[test]
    fn builds_itzg_environment_from_the_spec() {
        let env = env_for(&spec(false), "rcon-secret");
        assert!(env.contains(&"EULA=TRUE".to_string()));
        assert!(env.contains(&"TYPE=PAPER".to_string()));
        assert!(env.contains(&"VERSION=1.21.1".to_string()));
        assert!(env.contains(&"MEMORY=4G".to_string()));
        assert!(env.contains(&"ENABLE_RCON=TRUE".to_string()));
        assert!(env.contains(&"RCON_PORT=25575".to_string()));
        assert!(env.contains(&"RCON_PASSWORD=rcon-secret".to_string()));
        assert!(!env.contains(&"ONLINE_MODE=false".to_string()));

        let proxy_env = env_for(&spec(true), "rcon-secret");
        assert!(proxy_env.contains(&"ONLINE_MODE=false".to_string()));
    }

    #[test]
    fn rcon_port_and_password_are_deterministic_and_scoped() {
        let id = "01a1d37d-4700-7107-a4e8-72e044c3a464";
        let port = rcon_host_port(id);
        assert!((30_000..=39_999).contains(&port));
        assert_eq!(port, rcon_host_port(id));
        assert_ne!(port, rcon_host_port("01a1d37d-4700-7107-a4e8-72e044c3a465"));

        let password = rcon_password("node-secret", id);
        assert_eq!(password.len(), 64);
        assert_eq!(password, rcon_password("node-secret", id));
        assert_ne!(password, rcon_password("other-secret", id));
    }

    #[test]
    fn formats_memory_without_rounding_up() {
        assert_eq!(memory_env(1024), "1G");
        assert_eq!(memory_env(2048), "2G");
        assert_eq!(memory_env(1536), "1536M");
        assert_eq!(memory_bytes(1536), 1536 * 1024 * 1024);
    }

    #[test]
    fn binds_the_server_volume() {
        let config = host_config(&spec(false), "mc-net", "/srv/mc/");
        assert_eq!(
            config.binds,
            Some(vec![format!(
                "/srv/mc/{}/data:/data",
                "01a1d37d-4700-7107-a4e8-72e044c3a464"
            )])
        );
    }
}
