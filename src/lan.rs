use std::{
    collections::BTreeMap,
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail, ensure};
use if_addrs::IfAddr;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    cli::{
        HostTransport, JoinTransport, LanDiscoveryArgs, LanHostArgs, LanJoinArgs, LanNetworkArgs,
    },
    profile::ProfilePaths,
    runtime::{
        self, LINUX_RUFFLE_ARCHIVE_SHA256, Platform, RUFFLE_VERSION, WINDOWS_RUFFLE_ARCHIVE_SHA256,
    },
    source::SWF_SHA256,
    transport,
};

pub const PROTOCOL_VERSION: u16 = 2;
const WIRE_MAGIC: &str = "kof-wing-lan";
const MAX_PACKET_SIZE: usize = 8_192;
const MAX_UDP_DATAGRAM_SIZE: usize = 65_535;
const FUTURE_CLOCK_TOLERANCE_MS: u64 = 5_000;
const HOST_PACKET_MAX_AGE_MS: u64 = 5_000;
const REPLAY_GUARD_MAX_ENTRIES: usize = 256;
const NONCE_LENGTH: usize = 32;
const SESSION_ID_LENGTH: usize = 32;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Compatibility {
    pub swf_sha256: String,
    pub runtime_sha256: String,
    pub profile_sha256: String,
    pub transport_sha256: String,
    pub session_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointMetadata {
    pub endpoint: SocketAddr,
    pub transport: String,
    pub format: String,
    pub status: CapabilityStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityStatus {
    Available,
    MetadataOnly,
    NotImplemented,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionDescriptor {
    pub session_id: String,
    pub name: String,
    pub negotiation_endpoint: SocketAddr,
    pub compatibility: Compatibility,
    pub authoritative_host: bool,
    pub remote_input: EndpointMetadata,
    pub media: EndpointMetadata,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum WireMessage {
    Probe {
        nonce: String,
    },
    Announcement {
        nonce: String,
        session: SessionDescriptor,
    },
    JoinRequest {
        nonce: String,
        session_id: Option<String>,
        compatibility: Compatibility,
    },
    JoinResponse {
        nonce: String,
        accepted: bool,
        reason: String,
        session: Option<SessionDescriptor>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Packet {
    magic: String,
    protocol_version: u16,
    sent_at_ms: u64,
    message: WireMessage,
}

#[derive(Clone, Debug)]
pub struct DiscoveredSession {
    pub session: SessionDescriptor,
    pub compatible: bool,
    pub incompatibility: Option<String>,
}

#[derive(Default)]
struct ReplayGuard {
    seen: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LocalIpv4Interface {
    name: String,
    address: Ipv4Addr,
    netmask: Ipv4Addr,
    prefix_len: u8,
    broadcast: Option<Ipv4Addr>,
    is_loopback: bool,
    is_oper_up: bool,
    is_point_to_point: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedInterface {
    name: String,
    address: Ipv4Addr,
    prefix_len: u8,
    broadcast: Option<Ipv4Addr>,
    is_loopback: bool,
}

impl ReplayGuard {
    fn accept(&mut self, nonce: &str) -> bool {
        self.accept_at(nonce, now_ms())
    }

    fn accept_at(&mut self, nonce: &str, now: u64) -> bool {
        self.seen
            .retain(|_, received_at| now.saturating_sub(*received_at) <= HOST_PACKET_MAX_AGE_MS);
        if self.seen.contains_key(nonce) || self.seen.len() >= REPLAY_GUARD_MAX_ENTRIES {
            return false;
        }
        self.seen.insert(nonce.to_owned(), now);
        true
    }
}

pub fn doctor(work: &Path, profile_name: &str, network: &LanNetworkArgs) {
    println!("LAN zero-service diagnostics:");
    let interface = match resolve_interface(network.interface, network.prefix_len) {
        Ok(interface) => {
            report_address(
                "interface",
                IpAddr::V4(interface.address),
                network.allow_public,
            );
            println!(
                "  [ok] interface metadata: {} {}/{}",
                interface.name, interface.address, interface.prefix_len
            );
            Some(interface)
        }
        Err(error) => {
            println!("  [missing] interface: {error:#}");
            None
        }
    };
    report_address("bind", network.bind, network.allow_public);

    for (label, port) in [
        ("discovery", network.discovery_port),
        ("negotiation", network.session_port),
    ] {
        match UdpSocket::bind(SocketAddr::new(network.bind, port)) {
            Ok(socket) => println!(
                "  [ok] {label} UDP bind: {}",
                socket
                    .local_addr()
                    .map_or_else(|_| "available".to_owned(), |address| address.to_string())
            ),
            Err(error) => println!("  [missing] {label} UDP bind: {error}"),
        }
    }

    match local_compatibility(work, profile_name) {
        Ok(contract) => {
            println!("  [ok] SWF contract SHA-256: {}", contract.swf_sha256);
            println!(
                "  [ok] runtime contract SHA-256: {}",
                contract.runtime_sha256
            );
            println!(
                "  [ok] profile contract SHA-256: {}",
                contract.profile_sha256
            );
            println!(
                "  [ok] session contract SHA-256: {}",
                contract.session_sha256
            );
        }
        Err(error) => println!("  [missing] compatibility contract: {error:#}"),
    }

    match Command::new("ffmpeg").arg("-version").output() {
        Ok(output) if output.status.success() => {
            println!("  [ok] ffmpeg available (not wired to LAN media)")
        }
        Ok(output) => println!("  [missing] ffmpeg failed with {}", output.status),
        Err(error) => println!("  [missing] ffmpeg unavailable: {error}"),
    }
    match Platform::current()
        .map_err(anyhow::Error::msg)
        .and_then(|platform| runtime::locate(work, platform))
    {
        Ok(path) => println!("  [ok] pinned host runtime: {}", path.display()),
        Err(error) => println!("  [missing] pinned host runtime: {error:#}"),
    }
    match transport::verify_archives(work) {
        Ok(()) => println!("  [ok] pinned Sunshine/Moonlight transport archives"),
        Err(error) => println!("  [missing] pinned transport archives: {error:#}"),
    }
    if let Some(interface) = interface {
        match interface.broadcast {
            Some(address) => println!(
                "  [ok] /{} directed discovery broadcast: {address}:{}",
                interface.prefix_len, network.discovery_port
            ),
            None => println!(
                "  [missing] discovery broadcast: interface {} has no broadcast address",
                interface.name
            ),
        }
    }
    println!("  [implemented] discovery, negotiation, and optional Sunshine/Moonlight transport");
    println!("  [not verified] physical two-PC gameplay and reconnect behavior");
    println!("LAN gameplay readiness: unavailable until the physical two-PC gate passes");
}

pub fn discover(
    work: &Path,
    profile_name: &str,
    args: &LanDiscoveryArgs,
) -> Result<Vec<DiscoveredSession>> {
    let interface = checked_interface(&args.network)?;
    let local = local_compatibility(work, profile_name)?;
    discover_with_contract(&interface, &local, args)
}

pub fn host(work: &Path, profile_name: &str, args: &LanHostArgs) -> Result<()> {
    ensure!(
        !args.name.trim().is_empty() && args.name.trim().len() <= 64,
        "session name must be 1-64 bytes"
    );
    ensure!(
        !(args.once && args.transport == HostTransport::Sunshine),
        "--once is negotiation-only and cannot be combined with --transport sunshine"
    );
    ensure_distinct_ports(args)?;
    let interface = checked_interface(&args.network)?;
    ensure!(
        args.transport != HostTransport::Sunshine || address_allowed(interface.address.into()),
        "Sunshine transport cannot use --allow-public; select a private, loopback, or link-local interface"
    );
    let compatibility = local_compatibility(work, profile_name)?;
    let descriptor = session_descriptor(interface.address, &compatibility, args)?;
    let discovery = bind_socket(args.network.bind, args.network.discovery_port, "discovery")?;
    let negotiation = bind_socket(args.network.bind, args.network.session_port, "negotiation")?;
    let _transport = if args.transport == HostTransport::Sunshine {
        Some(transport::start_host(
            work,
            profile_name,
            interface.address.into(),
        )?)
    } else {
        None
    };
    discovery.set_nonblocking(true)?;
    negotiation.set_nonblocking(true)?;

    println!("authoritative host session: {}", descriptor.session_id);
    println!(
        "discovery: {}:{} ({}/{})",
        interface.address, args.network.discovery_port, interface.name, interface.prefix_len
    );
    println!("negotiation: {}", descriptor.negotiation_endpoint);
    println!("session contract SHA-256: {}", compatibility.session_sha256);
    if args.transport == HostTransport::Sunshine {
        println!("pinned Sunshine transport started; enter the one-time Moonlight PIN here");
    } else {
        println!("remote input/media transport not started (use --transport sunshine)");
    }

    let mut buffer = [0_u8; MAX_UDP_DATAGRAM_SIZE];
    let mut replay_guard = ReplayGuard::default();
    loop {
        if let Err(error) = receive_probe(
            &discovery,
            &mut buffer,
            interface.address,
            interface.prefix_len,
            args,
            &descriptor,
        ) {
            eprintln!("ignored discovery request error: {error:#}");
        }
        match receive_join(
            &negotiation,
            &mut buffer,
            interface.address,
            interface.prefix_len,
            args,
            &descriptor,
            &mut replay_guard,
        ) {
            Ok(true) if args.once => return Ok(()),
            Ok(_) => {}
            Err(error) => eprintln!("ignored negotiation request error: {error:#}"),
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub fn join(work: &Path, profile_name: &str, args: &LanJoinArgs) -> Result<SessionDescriptor> {
    let interface = checked_interface(&args.discovery.network)?;
    ensure!(
        args.transport != JoinTransport::Moonlight || address_allowed(interface.address.into()),
        "Moonlight transport cannot use --allow-public; select a private, loopback, or link-local interface"
    );
    let local = local_compatibility(work, profile_name)?;
    let (endpoint, expected_session_id) = if let Some(endpoint) = args.host {
        ensure!(
            endpoint.port() != 0,
            "host negotiation port must be non-zero"
        );
        validate_peer(
            endpoint.ip(),
            interface.address,
            interface.prefix_len,
            args.discovery.network.allow_public,
        )?;
        (endpoint, None)
    } else {
        let compatible: Vec<_> = discover_with_contract(&interface, &local, &args.discovery)?
            .into_iter()
            .filter(|result| result.compatible)
            .collect();
        ensure!(
            compatible.len() == 1,
            "automatic join requires exactly one compatible discovered session; found {}",
            compatible.len()
        );
        (
            compatible[0].session.negotiation_endpoint,
            Some(compatible[0].session.session_id.clone()),
        )
    };
    let session = negotiate(
        interface.address,
        interface.prefix_len,
        endpoint,
        expected_session_id.as_deref(),
        &local,
        args,
    )?;
    ensure!(
        args.transport != JoinTransport::Moonlight || descriptor_has_sunshine(&session),
        "host negotiated successfully but did not advertise Sunshine transport"
    );
    Ok(session)
}

pub fn local_compatibility(work: &Path, profile_name: &str) -> Result<Compatibility> {
    let profile = ProfilePaths::discover(profile_name)?;
    let config = profile.require_config()?;
    let input = profile.require_input()?;
    let language = crate::i18n::compatibility_contract(&config.language, &profile.catalogs)?;
    let base = work.join("extracted/kof-wing-ex-v1.0.swf");
    let embedded = crate::i18n::select_embedded_swf(work, &config.language.id, &base)?;
    let swf_sha256 = if embedded.active {
        crate::source::sha256(&embedded.swf)?
    } else {
        SWF_SHA256.to_owned()
    };
    Ok(compatibility_from_profile(
        canonical_profile(&config, &input, &language)?.as_bytes(),
        &swf_sha256,
    ))
}

fn canonical_profile(
    config: &crate::config::ProfileConfig,
    input: &crate::input::InputConfig,
    language_contract: &str,
) -> Result<String> {
    let canonical_profile = format!(
        "{}\n--- language ---\n{}\n--- input-v{} ---\n{}",
        toml::to_string(&config).context("cannot canonicalize profile config")?,
        language_contract,
        input.version,
        input
            .canonical_toml()
            .context("cannot canonicalize input config")?
    );
    Ok(canonical_profile)
}

fn compatibility_from_profile(profile: &[u8], swf_sha256: &str) -> Compatibility {
    let runtime_sha256 = sha256_bytes(runtime_contract().as_bytes());
    let profile_sha256 = sha256_bytes(profile);
    let transport_sha256 = sha256_bytes(transport::contract().as_bytes());
    let session_sha256 = session_contract_hash(
        swf_sha256,
        &runtime_sha256,
        &profile_sha256,
        &transport_sha256,
    );
    Compatibility {
        swf_sha256: swf_sha256.to_owned(),
        runtime_sha256,
        profile_sha256,
        transport_sha256,
        session_sha256,
    }
}

fn runtime_contract() -> String {
    format!(
        "ruffle={RUFFLE_VERSION};linux-archive={LINUX_RUFFLE_ARCHIVE_SHA256};windows-archive={WINDOWS_RUFFLE_ARCHIVE_SHA256};power=high;quality=high16x16;open-url=deny;filesystem=allow;storage=disk"
    )
}

fn session_descriptor(
    interface: Ipv4Addr,
    compatibility: &Compatibility,
    args: &LanHostArgs,
) -> Result<SessionDescriptor> {
    Ok(SessionDescriptor {
        session_id: random_hex(SESSION_ID_LENGTH / 2)?,
        name: args.name.trim().to_owned(),
        negotiation_endpoint: SocketAddr::new(interface.into(), args.network.session_port),
        compatibility: compatibility.clone(),
        authoritative_host: true,
        remote_input: if args.transport == HostTransport::Sunshine {
            transport_endpoint(interface)
        } else {
            EndpointMetadata {
                endpoint: SocketAddr::new(interface.into(), args.input_port),
                transport: "udp".to_owned(),
                format: "remote-player-input-v1".to_owned(),
                status: CapabilityStatus::NotImplemented,
            }
        },
        media: if args.transport == HostTransport::Sunshine {
            transport_endpoint(interface)
        } else {
            EndpointMetadata {
                endpoint: SocketAddr::new(interface.into(), args.media_port),
                transport: "udp".to_owned(),
                format: "low-latency-media-unselected".to_owned(),
                status: CapabilityStatus::MetadataOnly,
            }
        },
    })
}

fn transport_endpoint(interface: Ipv4Addr) -> EndpointMetadata {
    EndpointMetadata {
        endpoint: SocketAddr::new(interface.into(), transport::SUNSHINE_PORT),
        transport: "sunshine/moonlight".to_owned(),
        format: format!(
            "sunshine-{}+moonlight-{}",
            transport::SUNSHINE_VERSION,
            transport::MOONLIGHT_VERSION
        ),
        status: CapabilityStatus::Available,
    }
}

fn descriptor_has_sunshine(session: &SessionDescriptor) -> bool {
    session.remote_input.status == CapabilityStatus::Available
        && session.media.status == CapabilityStatus::Available
        && session.remote_input.transport == "sunshine/moonlight"
        && session.media.transport == "sunshine/moonlight"
}

fn discover_with_contract(
    interface: &ResolvedInterface,
    local: &Compatibility,
    args: &LanDiscoveryArgs,
) -> Result<Vec<DiscoveredSession>> {
    let client_bind = client_bind_address(args.network.bind, interface.address)?;
    let socket = UdpSocket::bind(SocketAddr::new(client_bind, 0))
        .with_context(|| format!("cannot bind discovery client to {client_bind}:0"))?;
    socket.set_broadcast(true)?;
    let timeout = Duration::from_millis(args.timeout_ms);
    socket.set_read_timeout(Some(timeout))?;
    let nonce = nonce()?;
    let probe = packet(WireMessage::Probe {
        nonce: nonce.clone(),
    });
    let target = SocketAddr::new(
        interface
            .broadcast
            .context("selected interface has no directed broadcast address")?
            .into(),
        args.network.discovery_port,
    );
    send_packet(&socket, target, &probe)?;

    let started = Instant::now();
    let mut sessions = BTreeMap::new();
    let mut buffer = [0_u8; MAX_UDP_DATAGRAM_SIZE];
    while let Some(remaining) = timeout.checked_sub(started.elapsed()) {
        socket.set_read_timeout(Some(remaining))?;
        match socket.recv_from(&mut buffer) {
            Ok((size, source)) => {
                let received = match decode_packet(&buffer[..size]) {
                    Ok(received) => received,
                    Err(_) => continue,
                };
                if validate_freshness(&received, args.stale_after_ms).is_err() {
                    continue;
                }
                let WireMessage::Announcement {
                    nonce: response_nonce,
                    session,
                } = received.message
                else {
                    continue;
                };
                if response_nonce != nonce {
                    continue;
                }
                if validate_peer(
                    source.ip(),
                    interface.address,
                    interface.prefix_len,
                    args.network.allow_public,
                )
                .is_err()
                    || session.negotiation_endpoint.ip() != source.ip()
                    || validate_descriptor(&session).is_err()
                {
                    continue;
                }
                let incompatibility = compatibility_difference(local, &session.compatibility);
                sessions.insert(
                    session.session_id.clone(),
                    DiscoveredSession {
                        compatible: incompatibility.is_none(),
                        incompatibility,
                        session,
                    },
                );
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                break;
            }
            Err(error) => return Err(error).context("cannot receive discovery announcement"),
        }
    }
    Ok(sessions.into_values().collect())
}

fn negotiate(
    interface: Ipv4Addr,
    prefix_len: u8,
    endpoint: SocketAddr,
    expected_session_id: Option<&str>,
    local: &Compatibility,
    args: &LanJoinArgs,
) -> Result<SessionDescriptor> {
    let client_bind = client_bind_address(args.discovery.network.bind, interface)?;
    let socket = UdpSocket::bind(SocketAddr::new(client_bind, 0))
        .with_context(|| format!("cannot bind negotiation client to {}:0", client_bind))?;
    let timeout = Duration::from_millis(args.discovery.timeout_ms);
    socket.set_read_timeout(Some(timeout))?;
    let nonce = nonce()?;
    send_packet(
        &socket,
        endpoint,
        &packet(WireMessage::JoinRequest {
            nonce: nonce.clone(),
            session_id: expected_session_id.map(str::to_owned),
            compatibility: local.clone(),
        }),
    )?;
    let mut buffer = [0_u8; MAX_UDP_DATAGRAM_SIZE];
    let started = Instant::now();
    while let Some(remaining) = timeout.checked_sub(started.elapsed()) {
        socket.set_read_timeout(Some(remaining))?;
        let (size, source) = match socket.recv_from(&mut buffer) {
            Ok(received) => received,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                break;
            }
            Err(error) => return Err(error).context("cannot receive host negotiation response"),
        };
        if source != endpoint {
            continue;
        }
        let Ok(response) = decode_packet(&buffer[..size]) else {
            continue;
        };
        if validate_freshness(&response, args.discovery.stale_after_ms).is_err() {
            continue;
        }
        let WireMessage::JoinResponse {
            nonce: response_nonce,
            accepted,
            reason,
            session,
        } = response.message
        else {
            continue;
        };
        if response_nonce != nonce {
            continue;
        }
        ensure!(accepted, "host rejected session contract: {reason}");
        let session = session.context("accepted negotiation omitted the session descriptor")?;
        validate_descriptor(&session)?;
        ensure!(
            session.negotiation_endpoint == endpoint,
            "host descriptor endpoint {} differs from contacted endpoint {endpoint}",
            session.negotiation_endpoint
        );
        for advertised in [
            session.negotiation_endpoint,
            session.remote_input.endpoint,
            session.media.endpoint,
        ] {
            validate_peer(
                advertised.ip(),
                interface,
                prefix_len,
                args.discovery.network.allow_public,
            )?;
        }
        if let Some(expected) = expected_session_id {
            ensure!(
                session.session_id == expected,
                "discovered session {expected} was replaced by {}",
                session.session_id
            );
        }
        if let Some(reason) = compatibility_difference(local, &session.compatibility) {
            bail!("host accepted but returned an incompatible contract: {reason}");
        }
        return Ok(session);
    }
    bail!("timed out waiting for a valid host negotiation response")
}

fn receive_probe(
    socket: &UdpSocket,
    buffer: &mut [u8],
    interface: Ipv4Addr,
    prefix_len: u8,
    args: &LanHostArgs,
    session: &SessionDescriptor,
) -> Result<()> {
    let Some((size, source)) = receive_nonblocking(socket, buffer, "discovery probe")? else {
        return Ok(());
    };
    let request = match decode_packet(&buffer[..size]) {
        Ok(request) => request,
        Err(_) => return Ok(()),
    };
    if validate_freshness(&request, HOST_PACKET_MAX_AGE_MS).is_err() {
        return Ok(());
    }
    let WireMessage::Probe { nonce } = request.message else {
        return Ok(());
    };
    if !valid_nonce(&nonce) {
        return Ok(());
    }
    if validate_peer(
        source.ip(),
        interface,
        prefix_len,
        args.network.allow_public,
    )
    .is_err()
    {
        return Ok(());
    }
    send_packet(
        socket,
        source,
        &packet(WireMessage::Announcement {
            nonce,
            session: session.clone(),
        }),
    )
}

fn receive_join(
    socket: &UdpSocket,
    buffer: &mut [u8],
    interface: Ipv4Addr,
    prefix_len: u8,
    args: &LanHostArgs,
    session: &SessionDescriptor,
    replay_guard: &mut ReplayGuard,
) -> Result<bool> {
    let Some((size, source)) = receive_nonblocking(socket, buffer, "join request")? else {
        return Ok(false);
    };
    let request = match decode_packet(&buffer[..size]) {
        Ok(request) => request,
        Err(_) => return Ok(false),
    };
    if validate_freshness(&request, HOST_PACKET_MAX_AGE_MS).is_err() {
        return Ok(false);
    }
    let WireMessage::JoinRequest {
        nonce,
        session_id,
        compatibility,
    } = request.message
    else {
        return Ok(false);
    };
    if !valid_nonce(&nonce) {
        return Ok(false);
    }
    if validate_peer(
        source.ip(),
        interface,
        prefix_len,
        args.network.allow_public,
    )
    .is_err()
    {
        return Ok(false);
    }
    if !replay_guard.accept(&nonce) {
        return Ok(false);
    }
    let difference = compatibility_difference(&session.compatibility, &compatibility);
    let contract_error = validate_compatibility(&compatibility)
        .err()
        .map(|error| format!("invalid compatibility contract: {error:#}"));
    let session_error = session_id
        .filter(|expected| expected != &session.session_id)
        .map(|expected| format!("requested session {expected} is no longer active"));
    let rejection = contract_error.or(session_error).or(difference);
    let accepted = rejection.is_none();
    let reason = rejection
        .unwrap_or_else(|| "compatibility hashes and capability contract match".to_owned());
    send_packet(
        socket,
        source,
        &packet(WireMessage::JoinResponse {
            nonce,
            accepted,
            reason,
            session: accepted.then(|| session.clone()),
        }),
    )?;
    Ok(accepted)
}

fn receive_nonblocking(
    socket: &UdpSocket,
    buffer: &mut [u8],
    context: &str,
) -> Result<Option<(usize, SocketAddr)>> {
    match socket.recv_from(buffer) {
        Ok(result) => Ok(Some(result)),
        Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(None),
        Err(error) => Err(error).with_context(|| format!("cannot receive {context}")),
    }
}

fn packet(message: WireMessage) -> Packet {
    Packet {
        magic: WIRE_MAGIC.to_owned(),
        protocol_version: PROTOCOL_VERSION,
        sent_at_ms: now_ms(),
        message,
    }
}

fn send_packet(socket: &UdpSocket, target: SocketAddr, message: &Packet) -> Result<()> {
    let bytes = serde_json::to_vec(message).context("cannot encode LAN packet")?;
    ensure!(
        bytes.len() <= MAX_PACKET_SIZE,
        "LAN packet exceeds {MAX_PACKET_SIZE} bytes"
    );
    socket
        .send_to(&bytes, target)
        .with_context(|| format!("cannot send LAN packet to {target}"))?;
    Ok(())
}

fn decode_packet(bytes: &[u8]) -> Result<Packet> {
    ensure!(!bytes.is_empty(), "empty LAN packet");
    ensure!(
        bytes.len() <= MAX_PACKET_SIZE,
        "LAN packet exceeds {MAX_PACKET_SIZE} bytes"
    );
    let packet: Packet = serde_json::from_slice(bytes).context("invalid LAN packet")?;
    ensure!(packet.magic == WIRE_MAGIC, "invalid LAN packet magic");
    ensure!(
        packet.protocol_version == PROTOCOL_VERSION,
        "unsupported LAN protocol version {} (expected {PROTOCOL_VERSION})",
        packet.protocol_version
    );
    Ok(packet)
}

fn validate_freshness(packet: &Packet, stale_after_ms: u64) -> Result<()> {
    let now = now_ms();
    ensure!(
        packet.sent_at_ms <= now.saturating_add(FUTURE_CLOCK_TOLERANCE_MS),
        "LAN packet timestamp is too far in the future"
    );
    ensure!(
        now.saturating_sub(packet.sent_at_ms) <= stale_after_ms,
        "stale LAN packet"
    );
    Ok(())
}

fn validate_descriptor(session: &SessionDescriptor) -> Result<()> {
    ensure!(
        session.authoritative_host,
        "peer did not declare an authoritative host"
    );
    ensure!(
        session.session_id.len() == SESSION_ID_LENGTH
            && session
                .session_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
        "session ID must be {SESSION_ID_LENGTH} hexadecimal characters"
    );
    ensure!(
        !session.name.trim().is_empty() && session.name.len() <= 64,
        "session name must be 1-64 bytes"
    );
    validate_compatibility(&session.compatibility)?;
    ensure!(
        session.negotiation_endpoint.port() != 0
            && session.remote_input.endpoint.port() != 0
            && session.media.endpoint.port() != 0,
        "session contains a zero endpoint port"
    );
    ensure!(
        session.remote_input.endpoint.ip() == session.negotiation_endpoint.ip()
            && session.media.endpoint.ip() == session.negotiation_endpoint.ip(),
        "session endpoint addresses disagree"
    );
    let negotiation_only = session.remote_input.status == CapabilityStatus::NotImplemented
        && session.media.status == CapabilityStatus::MetadataOnly
        && session.remote_input.transport == "udp"
        && session.remote_input.format == "remote-player-input-v1"
        && session.media.transport == "udp"
        && session.media.format == "low-latency-media-unselected";
    let transport_format = format!(
        "sunshine-{}+moonlight-{}",
        transport::SUNSHINE_VERSION,
        transport::MOONLIGHT_VERSION
    );
    let sunshine = session.remote_input.status == CapabilityStatus::Available
        && session.media.status == CapabilityStatus::Available
        && session.remote_input.endpoint.port() == transport::SUNSHINE_PORT
        && session.media.endpoint.port() == transport::SUNSHINE_PORT
        && session.remote_input.transport == "sunshine/moonlight"
        && session.media.transport == "sunshine/moonlight"
        && session.remote_input.format == transport_format
        && session.media.format == transport_format;
    ensure!(
        negotiation_only || sunshine,
        "unsupported input/media endpoint contract"
    );
    Ok(())
}

fn validate_compatibility(contract: &Compatibility) -> Result<()> {
    for (label, hash) in [
        ("SWF", contract.swf_sha256.as_str()),
        ("runtime", contract.runtime_sha256.as_str()),
        ("profile", contract.profile_sha256.as_str()),
        ("transport", contract.transport_sha256.as_str()),
        ("session", contract.session_sha256.as_str()),
    ] {
        ensure!(
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')),
            "{label} hash must be 64 lowercase hexadecimal characters"
        );
    }
    let expected_session = session_contract_hash(
        &contract.swf_sha256,
        &contract.runtime_sha256,
        &contract.profile_sha256,
        &contract.transport_sha256,
    );
    ensure!(
        contract.session_sha256 == expected_session,
        "session hash does not bind the advertised component hashes"
    );
    Ok(())
}

fn session_contract_hash(swf: &str, runtime: &str, profile: &str, transport: &str) -> String {
    sha256_bytes(
        format!(
            "protocol={PROTOCOL_VERSION};swf={swf};runtime={runtime};profile={profile};transport={transport};authority=host;input=sunshine;media=sunshine;reconnect=transport-managed"
        )
        .as_bytes(),
    )
}

fn compatibility_difference(expected: &Compatibility, actual: &Compatibility) -> Option<String> {
    let mut differences = Vec::new();
    if expected.swf_sha256 != actual.swf_sha256 {
        differences.push("SWF hash");
    }
    if expected.runtime_sha256 != actual.runtime_sha256 {
        differences.push("runtime hash");
    }
    if expected.profile_sha256 != actual.profile_sha256 {
        differences.push("profile hash");
    }
    if expected.transport_sha256 != actual.transport_sha256 {
        differences.push("transport hash");
    }
    if expected.session_sha256 != actual.session_sha256 {
        differences.push("session hash");
    }
    (!differences.is_empty()).then(|| format!("incompatible {}", differences.join(", ")))
}

pub fn address_allowed(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            address.is_private() || address.is_loopback() || address.is_link_local()
        }
        IpAddr::V6(address) => {
            address.is_unique_local() || address.is_loopback() || address.is_unicast_link_local()
        }
    }
}

fn checked_interface(network: &LanNetworkArgs) -> Result<ResolvedInterface> {
    ensure!(
        matches!(network.bind, IpAddr::V4(_)),
        "LAN bind address must be IPv4; use 0.0.0.0 or the selected IPv4 interface"
    );
    ensure!(
        network.discovery_port != 0 && network.session_port != 0,
        "discovery and negotiation ports must be non-zero"
    );
    let interface = resolve_interface(network.interface, network.prefix_len)?;
    ensure!(
        !interface.is_loopback,
        "loopback interface {} is diagnostic-only; select a broadcast-capable LAN interface",
        interface.address
    );
    validate_interface_policy(&interface, network.allow_public)?;
    if !network.bind.is_unspecified() {
        ensure!(
            network.allow_public || address_allowed(network.bind),
            "bind address {} is not private, loopback, or link-local",
            network.bind
        );
        ensure!(
            network.bind == IpAddr::V4(interface.address),
            "concrete bind address {} must match interface {}",
            network.bind,
            interface.address,
        );
    }
    Ok(interface)
}

fn validate_interface_policy(interface: &ResolvedInterface, allow_public: bool) -> Result<()> {
    ensure!(
        allow_public || address_allowed(interface.address.into()),
        "interface {} is not private, loopback, or link-local; pass --interface {} --allow-public to opt in",
        interface.address,
        interface.address
    );
    Ok(())
}

fn client_bind_address(configured: IpAddr, interface: Ipv4Addr) -> Result<IpAddr> {
    if configured.is_unspecified() {
        return Ok(interface.into());
    }
    ensure!(
        configured == IpAddr::V4(interface),
        "concrete bind address {configured} must match interface {interface}"
    );
    Ok(configured)
}

fn resolve_interface(
    configured: Option<IpAddr>,
    prefix_override: Option<u8>,
) -> Result<ResolvedInterface> {
    let interfaces = if_addrs::get_if_addrs()
        .context("cannot read local network interface metadata")?
        .into_iter()
        .filter_map(|interface| {
            let is_oper_up = interface.is_oper_up();
            let is_point_to_point = interface.is_p2p();
            let IfAddr::V4(address) = interface.addr else {
                return None;
            };
            Some(LocalIpv4Interface {
                name: interface.name,
                address: address.ip,
                netmask: address.netmask,
                prefix_len: address.prefixlen,
                broadcast: address.broadcast,
                is_loopback: address.ip.is_loopback(),
                is_oper_up,
                is_point_to_point,
            })
        })
        .collect::<Vec<_>>();
    let default_route = if configured.is_none() {
        Some(default_route_interface()?)
    } else {
        None
    };
    select_interface(configured, prefix_override, default_route, &interfaces)
}

fn default_route_interface() -> Result<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").context("cannot open interface probe socket")?;
    socket
        .connect("192.0.2.1:9")
        .context("cannot resolve a default local interface; pass --interface")?;
    let IpAddr::V4(address) = socket.local_addr()?.ip() else {
        bail!("default-route probe returned IPv6; pass --interface with a local IPv4 address")
    };
    Ok(address)
}

fn select_interface(
    configured: Option<IpAddr>,
    prefix_override: Option<u8>,
    default_route: Option<Ipv4Addr>,
    interfaces: &[LocalIpv4Interface],
) -> Result<ResolvedInterface> {
    let (address, automatic) = match configured {
        Some(IpAddr::V4(address)) => {
            ensure!(
                !address.is_unspecified(),
                "interface address cannot be unspecified"
            );
            (address, false)
        }
        Some(IpAddr::V6(_)) => bail!("LAN discovery currently requires an IPv4 interface"),
        None => (
            default_route.context(
                "cannot resolve the active default-route IPv4 interface; pass --interface",
            )?,
            true,
        ),
    };
    let metadata = interfaces
        .iter()
        .find(|interface| interface.address == address)
        .with_context(|| {
            if automatic {
                format!(
                    "default-route address {address} does not map to a local IPv4 interface; pass --interface"
                )
            } else {
                format!("interface address {address} is not assigned to this host")
            }
        })?;
    ensure!(
        metadata.is_oper_up,
        "interface {} ({address}) is not operationally up",
        metadata.name
    );
    if let Some(prefix) = prefix_override {
        ensure!(
            prefix == metadata.prefix_len,
            "--prefix-len {prefix} does not match interface {} actual prefix /{}",
            metadata.name,
            metadata.prefix_len
        );
    }
    let mask = subnet_mask(metadata.prefix_len)?;
    ensure!(
        metadata.netmask == Ipv4Addr::from(mask),
        "interface {} reports inconsistent IPv4 netmask {} and prefix /{}",
        metadata.name,
        metadata.netmask,
        metadata.prefix_len
    );
    if !metadata.is_loopback {
        ensure!(
            metadata.prefix_len < 32,
            "interface {} ({address}/{}) is not broadcast-capable; select an active LAN interface",
            metadata.name,
            metadata.prefix_len
        );
        ensure!(
            !metadata.is_point_to_point,
            "interface {} ({address}) is point-to-point, not a broadcast LAN interface",
            metadata.name
        );
        let broadcast = metadata.broadcast.with_context(|| {
            format!(
                "interface {} ({address}/{}) has no broadcast address; select an active LAN interface",
                metadata.name, metadata.prefix_len
            )
        })?;
        let expected = directed_broadcast(address, metadata.prefix_len)?;
        ensure!(
            broadcast == expected,
            "interface {} reports broadcast {broadcast}, inconsistent with {address}/{} ({expected})",
            metadata.name,
            metadata.prefix_len
        );
    }
    Ok(ResolvedInterface {
        name: metadata.name.clone(),
        address,
        prefix_len: metadata.prefix_len,
        broadcast: metadata.broadcast,
        is_loopback: metadata.is_loopback,
    })
}

fn validate_peer(
    peer: IpAddr,
    interface: Ipv4Addr,
    prefix_len: u8,
    allow_public: bool,
) -> Result<()> {
    ensure!(
        allow_public || address_allowed(peer),
        "peer {peer} is not private or local"
    );
    let IpAddr::V4(peer) = peer else {
        bail!("IPv6 peers are not supported by IPv4 discovery")
    };
    ensure!(
        same_subnet(interface, peer, prefix_len)?,
        "peer {peer} is outside {interface}/{prefix_len}"
    );
    Ok(())
}

fn same_subnet(left: Ipv4Addr, right: Ipv4Addr, prefix_len: u8) -> Result<bool> {
    let mask = subnet_mask(prefix_len)?;
    Ok(u32::from(left) & mask == u32::from(right) & mask)
}

fn directed_broadcast(interface: Ipv4Addr, prefix_len: u8) -> Result<Ipv4Addr> {
    let mask = subnet_mask(prefix_len)?;
    Ok(Ipv4Addr::from((u32::from(interface) & mask) | !mask))
}

fn subnet_mask(prefix_len: u8) -> Result<u32> {
    ensure!(prefix_len <= 32, "IPv4 prefix length must be 0-32");
    Ok(if prefix_len == 0 {
        0
    } else {
        u32::MAX << (32 - prefix_len)
    })
}

fn bind_socket(bind: IpAddr, port: u16, label: &str) -> Result<UdpSocket> {
    UdpSocket::bind(SocketAddr::new(bind, port))
        .with_context(|| format!("cannot bind {label} UDP socket to {bind}:{port}"))
}

fn ensure_distinct_ports(args: &LanHostArgs) -> Result<()> {
    let ports = [
        args.network.discovery_port,
        args.network.session_port,
        args.input_port,
        args.media_port,
    ];
    ensure!(
        ports.iter().all(|port| *port != 0),
        "host ports must be non-zero"
    );
    for (index, port) in ports.iter().enumerate() {
        ensure!(
            !ports[..index].contains(port),
            "discovery, negotiation, input, and media ports must be distinct"
        );
    }
    Ok(())
}

fn report_address(label: &str, address: IpAddr, allow_public: bool) {
    if address.is_unspecified() || allow_public || address_allowed(address) {
        println!("  [ok] {label} address policy: {address}");
    } else {
        println!("  [rejected] {label} address policy: {address} is public");
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn random_hex(byte_length: usize) -> Result<String> {
    let mut bytes = vec![0_u8; byte_length];
    getrandom::fill(&mut bytes).context("cannot obtain operating-system randomness")?;
    let mut encoded = String::with_capacity(byte_length * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(encoded)
}

fn nonce() -> Result<String> {
    random_hex(NONCE_LENGTH / 2)
}

fn valid_nonce(value: &str) -> bool {
    value.len() == NONCE_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn now_ms() -> u64 {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(milliseconds).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compatibility(profile: &[u8]) -> Compatibility {
        compatibility_from_profile(profile, SWF_SHA256)
    }

    fn loopback_host_args() -> LanHostArgs {
        LanHostArgs {
            network: LanNetworkArgs {
                bind: "127.0.0.1".parse().unwrap(),
                interface: Some("127.0.0.1".parse().unwrap()),
                prefix_len: Some(8),
                discovery_port: 27_880,
                session_port: 27_881,
                allow_public: false,
            },
            name: "fixture".to_owned(),
            input_port: 27_882,
            media_port: 27_883,
            once: true,
            transport: HostTransport::Negotiation,
        }
    }

    fn fixture_nonce(character: char) -> String {
        std::iter::repeat_n(character, NONCE_LENGTH).collect()
    }

    fn interface_fixture(
        name: &str,
        address: &str,
        prefix_len: u8,
        broadcast: Option<&str>,
    ) -> LocalIpv4Interface {
        LocalIpv4Interface {
            name: name.to_owned(),
            address: address.parse().unwrap(),
            netmask: Ipv4Addr::from(subnet_mask(prefix_len).unwrap()),
            prefix_len,
            broadcast: broadcast.map(|value| value.parse().unwrap()),
            is_loopback: address.starts_with("127."),
            is_oper_up: true,
            is_point_to_point: false,
        }
    }

    #[test]
    fn interface_selection_uses_actual_prefix_and_rejects_override_mismatch() {
        let interfaces = [interface_fixture(
            "wlan0",
            "10.42.5.7",
            22,
            Some("10.42.7.255"),
        )];
        let selected =
            select_interface(Some("10.42.5.7".parse().unwrap()), None, None, &interfaces).unwrap();
        assert_eq!(selected.prefix_len, 22);
        assert_eq!(selected.broadcast, Some("10.42.7.255".parse().unwrap()));

        let error = select_interface(
            Some("10.42.5.7".parse().unwrap()),
            Some(24),
            None,
            &interfaces,
        )
        .unwrap_err();
        assert!(error.to_string().contains("actual prefix /22"));
    }

    #[test]
    fn automatic_selection_rejects_32_tunnel_without_bridge_fallback() {
        let mut tunnel = interface_fixture("psiphon0", "172.31.255.1", 32, None);
        tunnel.is_point_to_point = true;
        let mut bridge = interface_fixture("virbr0", "192.168.122.1", 24, Some("192.168.122.255"));
        bridge.is_oper_up = false;
        let error = select_interface(
            None,
            None,
            Some("172.31.255.1".parse().unwrap()),
            &[tunnel, bridge],
        )
        .unwrap_err();
        assert!(error.to_string().contains("psiphon0"));
        assert!(error.to_string().contains("not broadcast-capable"));
    }

    #[test]
    fn automatic_selection_never_falls_back_to_inactive_private_bridge() {
        let mut bridge = interface_fixture("docker0", "172.17.0.1", 16, Some("172.17.255.255"));
        bridge.is_oper_up = false;
        let error = select_interface(None, None, Some("203.0.113.9".parse().unwrap()), &[bridge])
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("default-route address 203.0.113.9")
        );
    }

    #[test]
    fn explicit_interface_must_be_a_local_address() {
        let interfaces = [interface_fixture(
            "wlan0",
            "192.168.1.20",
            24,
            Some("192.168.1.255"),
        )];
        let error = select_interface(
            Some("192.168.1.99".parse().unwrap()),
            None,
            None,
            &interfaces,
        )
        .unwrap_err();
        assert!(error.to_string().contains("is not assigned to this host"));
    }

    #[test]
    fn explicit_loopback_remains_available_for_diagnostics() {
        let interfaces = [interface_fixture("lo", "127.0.0.1", 8, None)];
        let selected =
            select_interface(Some("127.0.0.1".parse().unwrap()), None, None, &interfaces).unwrap();
        assert!(selected.is_loopback);
        assert_eq!(selected.prefix_len, 8);
        assert_eq!(selected.broadcast, None);
    }

    #[test]
    fn public_broadcast_interface_requires_explicit_policy_opt_in() {
        let interfaces = [interface_fixture(
            "campus-wifi",
            "203.0.113.20",
            24,
            Some("203.0.113.255"),
        )];
        let selected = select_interface(
            Some("203.0.113.20".parse().unwrap()),
            None,
            None,
            &interfaces,
        )
        .unwrap();
        assert!(validate_interface_policy(&selected, false).is_err());
        assert!(validate_interface_policy(&selected, true).is_ok());
    }

    #[test]
    fn replay_guard_is_bounded_rejects_duplicates_and_prunes_expired_entries() {
        let mut guard = ReplayGuard::default();
        for index in 0..REPLAY_GUARD_MAX_ENTRIES {
            assert!(guard.accept_at(&format!("nonce-{index}"), 1));
        }
        assert_eq!(guard.seen.len(), REPLAY_GUARD_MAX_ENTRIES);
        assert!(!guard.accept_at("nonce-0", 1));
        assert!(!guard.accept_at("unseen-at-capacity", 1));
        assert_eq!(guard.seen.len(), REPLAY_GUARD_MAX_ENTRIES);

        let after_expiry = 1 + HOST_PACKET_MAX_AGE_MS + 1;
        assert!(guard.accept_at("unseen-after-expiry", after_expiry));
        assert_eq!(guard.seen.len(), 1);
        assert!(guard.seen.contains_key("unseen-after-expiry"));
    }

    #[test]
    fn packet_round_trip_and_malformed_rejection() {
        let original = packet(WireMessage::Probe {
            nonce: "fixture".to_owned(),
        });
        let encoded = serde_json::to_vec(&original).expect("encode fixture");
        assert_eq!(decode_packet(&encoded).expect("decode fixture"), original);
        assert!(decode_packet(&[]).is_err());
        assert!(decode_packet(b"not-json").is_err());
        assert!(decode_packet(&encoded[..encoded.len() - 1]).is_err());
        assert!(decode_packet(&vec![b'x'; MAX_PACKET_SIZE + 1]).is_err());

        let unknown_field = serde_json::json!({
            "magic": WIRE_MAGIC,
            "protocol_version": PROTOCOL_VERSION,
            "sent_at_ms": now_ms(),
            "message": {"type": "probe", "nonce": "fixture", "extra": true}
        });
        assert!(decode_packet(&serde_json::to_vec(&unknown_field).unwrap()).is_err());

        let wrong_version = serde_json::json!({
            "magic": WIRE_MAGIC,
            "protocol_version": PROTOCOL_VERSION + 1,
            "sent_at_ms": now_ms(),
            "message": {"type": "probe", "nonce": "fixture"}
        });
        assert!(decode_packet(&serde_json::to_vec(&wrong_version).unwrap()).is_err());
    }

    #[test]
    fn rejects_every_incompatible_contract_hash() {
        let expected = compatibility(b"profile-a");
        let mut actual = expected.clone();
        actual.profile_sha256 = "different".to_owned();
        assert_eq!(
            compatibility_difference(&expected, &actual).as_deref(),
            Some("incompatible profile hash")
        );

        actual = expected.clone();
        actual.runtime_sha256 = "different".to_owned();
        assert!(
            compatibility_difference(&expected, &actual)
                .expect("runtime mismatch")
                .contains("runtime hash")
        );

        actual = expected.clone();
        actual.swf_sha256 = "different".to_owned();
        assert!(
            compatibility_difference(&expected, &actual)
                .expect("SWF mismatch")
                .contains("SWF hash")
        );

        actual = expected.clone();
        actual.transport_sha256 = "different".to_owned();
        assert!(
            compatibility_difference(&expected, &actual)
                .expect("transport mismatch")
                .contains("transport hash")
        );

        actual = expected.clone();
        actual.session_sha256 = "different".to_owned();
        assert!(
            compatibility_difference(&expected, &actual)
                .expect("session mismatch")
                .contains("session hash")
        );
    }

    #[test]
    fn host_negotiation_rejects_incompatible_client_packet() {
        let args = loopback_host_args();
        let host_contract = compatibility(b"host-profile");
        let descriptor =
            session_descriptor("127.0.0.1".parse().unwrap(), &host_contract, &args).unwrap();
        let host = UdpSocket::bind("127.0.0.1:0").expect("bind host fixture");
        host.set_nonblocking(true).unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").expect("bind client fixture");
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        send_packet(
            &client,
            host.local_addr().unwrap(),
            &packet(WireMessage::JoinRequest {
                nonce: fixture_nonce('a'),
                session_id: Some(descriptor.session_id.clone()),
                compatibility: compatibility(b"different-profile"),
            }),
        )
        .unwrap();

        let mut buffer = [0_u8; MAX_UDP_DATAGRAM_SIZE];
        let mut replay_guard = ReplayGuard::default();
        assert!(
            !receive_join(
                &host,
                &mut buffer,
                "127.0.0.1".parse().unwrap(),
                8,
                &args,
                &descriptor,
                &mut replay_guard,
            )
            .unwrap()
        );
        let (size, _) = client.recv_from(&mut buffer).expect("receive rejection");
        let response = decode_packet(&buffer[..size]).expect("decode rejection");
        let WireMessage::JoinResponse {
            accepted,
            reason,
            session,
            ..
        } = response.message
        else {
            panic!("expected join response")
        };
        assert!(!accepted);
        assert!(reason.contains("profile hash"));
        assert!(reason.contains("session hash"));
        assert!(session.is_none());
    }

    #[test]
    fn host_discovery_and_successful_negotiation_use_udp() {
        let args = loopback_host_args();
        let contract = compatibility(b"shared-profile");
        let descriptor =
            session_descriptor("127.0.0.1".parse().unwrap(), &contract, &args).unwrap();
        let host = UdpSocket::bind("127.0.0.1:0").expect("bind host fixture");
        host.set_nonblocking(true).unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").expect("bind client fixture");
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut buffer = [0_u8; MAX_UDP_DATAGRAM_SIZE];
        let mut replay_guard = ReplayGuard::default();

        send_packet(
            &client,
            host.local_addr().unwrap(),
            &packet(WireMessage::Probe {
                nonce: fixture_nonce('a'),
            }),
        )
        .unwrap();
        receive_probe(
            &host,
            &mut buffer,
            "127.0.0.1".parse().unwrap(),
            8,
            &args,
            &descriptor,
        )
        .unwrap();
        let (size, _) = client.recv_from(&mut buffer).expect("receive announcement");
        assert!(matches!(
            decode_packet(&buffer[..size]).unwrap().message,
            WireMessage::Announcement { session, .. } if session == descriptor
        ));

        send_packet(
            &client,
            host.local_addr().unwrap(),
            &packet(WireMessage::JoinRequest {
                nonce: "too-short".to_owned(),
                session_id: Some(descriptor.session_id.clone()),
                compatibility: contract.clone(),
            }),
        )
        .unwrap();
        assert!(
            !receive_join(
                &host,
                &mut buffer,
                "127.0.0.1".parse().unwrap(),
                8,
                &args,
                &descriptor,
                &mut replay_guard,
            )
            .unwrap()
        );

        let replay_client = UdpSocket::bind("127.0.0.1:0").expect("bind replay fixture");
        send_packet(
            &client,
            host.local_addr().unwrap(),
            &packet(WireMessage::JoinRequest {
                nonce: fixture_nonce('b'),
                session_id: Some(descriptor.session_id.clone()),
                compatibility: contract.clone(),
            }),
        )
        .unwrap();
        assert!(
            receive_join(
                &host,
                &mut buffer,
                "127.0.0.1".parse().unwrap(),
                8,
                &args,
                &descriptor,
                &mut replay_guard,
            )
            .unwrap()
        );
        let (size, _) = client.recv_from(&mut buffer).expect("receive acceptance");
        let response = decode_packet(&buffer[..size]).expect("decode acceptance");
        assert!(matches!(
            response.message,
            WireMessage::JoinResponse {
                accepted: true,
                session: Some(session),
                ..
            } if session == descriptor
        ));

        send_packet(
            &replay_client,
            host.local_addr().unwrap(),
            &packet(WireMessage::JoinRequest {
                nonce: fixture_nonce('b'),
                session_id: Some(descriptor.session_id.clone()),
                compatibility: contract,
            }),
        )
        .unwrap();
        assert!(
            !receive_join(
                &host,
                &mut buffer,
                "127.0.0.1".parse().unwrap(),
                8,
                &args,
                &descriptor,
                &mut replay_guard,
            )
            .unwrap()
        );
        replay_client.set_nonblocking(true).unwrap();
        assert_eq!(
            replay_client.recv_from(&mut buffer).unwrap_err().kind(),
            ErrorKind::WouldBlock
        );
    }

    #[test]
    fn host_silently_drops_stale_and_off_subnet_join_requests() {
        let args = loopback_host_args();
        let contract = compatibility(b"shared-profile");
        let descriptor =
            session_descriptor("127.0.0.1".parse().unwrap(), &contract, &args).unwrap();
        let host = UdpSocket::bind("127.0.0.1:0").expect("bind host fixture");
        host.set_nonblocking(true).unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").expect("bind client fixture");
        let mut buffer = [0_u8; MAX_UDP_DATAGRAM_SIZE];
        let mut replay_guard = ReplayGuard::default();

        let mut stale = packet(WireMessage::JoinRequest {
            nonce: fixture_nonce('c'),
            session_id: Some(descriptor.session_id.clone()),
            compatibility: contract.clone(),
        });
        stale.sent_at_ms = now_ms().saturating_sub(HOST_PACKET_MAX_AGE_MS + 1);
        send_packet(&client, host.local_addr().unwrap(), &stale).unwrap();
        assert!(
            !receive_join(
                &host,
                &mut buffer,
                "127.0.0.1".parse().unwrap(),
                8,
                &args,
                &descriptor,
                &mut replay_guard,
            )
            .unwrap()
        );

        send_packet(
            &client,
            host.local_addr().unwrap(),
            &packet(WireMessage::JoinRequest {
                nonce: fixture_nonce('d'),
                session_id: Some(descriptor.session_id.clone()),
                compatibility: contract,
            }),
        )
        .unwrap();
        assert!(
            !receive_join(
                &host,
                &mut buffer,
                "192.168.1.10".parse().unwrap(),
                8,
                &args,
                &descriptor,
                &mut replay_guard,
            )
            .unwrap()
        );

        client.set_nonblocking(true).unwrap();
        assert_eq!(
            client.recv_from(&mut buffer).unwrap_err().kind(),
            ErrorKind::WouldBlock
        );
    }

    #[test]
    fn private_address_policy_and_subnet_are_strict() {
        for allowed in ["10.0.0.1", "172.16.0.1", "192.168.1.1", "127.0.0.1"] {
            assert!(address_allowed(allowed.parse().unwrap()));
        }
        assert!(!address_allowed("8.8.8.8".parse().unwrap()));
        assert!(
            validate_peer(
                "192.168.10.22".parse().unwrap(),
                "192.168.10.4".parse().unwrap(),
                24,
                false
            )
            .is_ok()
        );
        assert!(
            validate_peer(
                "192.168.11.22".parse().unwrap(),
                "192.168.10.4".parse().unwrap(),
                24,
                false
            )
            .is_err()
        );
        assert!(
            validate_peer(
                "8.8.8.8".parse().unwrap(),
                "8.8.8.1".parse().unwrap(),
                24,
                false
            )
            .is_err()
        );

        let mut args = loopback_host_args();
        args.network.bind = "::".parse().unwrap();
        assert!(checked_interface(&args.network).is_err());
        args.network.bind = "127.0.0.1".parse().unwrap();
        args.network.discovery_port = 0;
        assert!(checked_interface(&args.network).is_err());
    }

    #[test]
    fn compatibility_hash_binds_pinned_runtimes_and_capability_policy() {
        let runtime = runtime_contract();
        for required in [
            LINUX_RUFFLE_ARCHIVE_SHA256,
            WINDOWS_RUFFLE_ARCHIVE_SHA256,
            "open-url=deny",
            "filesystem=allow",
            "storage=disk",
        ] {
            assert!(runtime.contains(required));
        }

        let contract = compatibility(b"profile");
        let legacy_session_hash = sha256_bytes(
            format!(
                "protocol={PROTOCOL_VERSION};swf={};runtime={};profile={}",
                contract.swf_sha256, contract.runtime_sha256, contract.profile_sha256
            )
            .as_bytes(),
        );
        assert_ne!(contract.session_sha256, legacy_session_hash);
        validate_compatibility(&contract).unwrap();

        let patched = compatibility_from_profile(b"profile", "synthetic-patched-swf-hash");
        assert_eq!(patched.swf_sha256, "synthetic-patched-swf-hash");
        assert_ne!(patched.session_sha256, contract.session_sha256);
    }

    #[test]
    fn sunshine_descriptor_is_an_exact_supported_transport_contract() {
        let mut args = loopback_host_args();
        args.transport = HostTransport::Sunshine;
        args.once = false;
        let descriptor = session_descriptor(Ipv4Addr::LOCALHOST, &compatibility(b"profile"), &args)
            .expect("Sunshine descriptor");
        validate_descriptor(&descriptor).expect("supported descriptor");
        assert_eq!(
            descriptor.remote_input.endpoint.port(),
            transport::SUNSHINE_PORT
        );
        assert_eq!(descriptor.media.status, CapabilityStatus::Available);
        assert!(descriptor_has_sunshine(&descriptor));
    }

    #[test]
    fn moonlight_transport_rejects_negotiation_only_descriptor() {
        let descriptor = session_descriptor(
            Ipv4Addr::LOCALHOST,
            &compatibility(b"profile"),
            &loopback_host_args(),
        )
        .expect("negotiation descriptor");
        assert!(!descriptor_has_sunshine(&descriptor));
    }

    #[test]
    fn stale_and_future_packets_are_rejected() {
        let mut old = packet(WireMessage::Probe {
            nonce: "old".to_owned(),
        });
        old.sent_at_ms = now_ms().saturating_sub(10_000);
        assert!(validate_freshness(&old, 100).is_err());

        let mut future = old;
        future.sent_at_ms = now_ms().saturating_add(FUTURE_CLOCK_TOLERANCE_MS + 1);
        assert!(validate_freshness(&future, 100).is_err());
    }

    #[test]
    fn profile_hash_is_canonical_and_changes_session_hash() {
        let make_input = |bindings: &[(&str, &str)]| {
            let mut input = crate::input::InputConfig::default();
            for (button, target) in bindings {
                input
                    .set_binding(
                        "p1",
                        "pad",
                        crate::input::DeviceKind::Gamepad,
                        crate::input::FieldUpdate::Keep,
                        crate::input::FieldUpdate::Keep,
                        crate::input::Binding {
                            source: format!("button:{button}").parse().unwrap(),
                            target: (*target).to_owned(),
                            chord: vec![],
                        },
                    )
                    .unwrap();
            }
            input
        };
        let profile = crate::config::ProfileConfig::default();
        let first_input = make_input(&[("south", "j"), ("east", "k")]);
        let same_input = make_input(&[("EAST", "K"), ("SOUTH", "J")]);
        let changed_input = make_input(&[("south", "j"), ("east", "l")]);
        let english_contract =
            r#"{"language":"en","catalog_version":1,"messages":{"menu.play":"Play"}}"#;
        let french_contract =
            r#"{"language":"fr","catalog_version":1,"messages":{"menu.play":"Jouer"}}"#;
        let first = compatibility(
            canonical_profile(&profile, &first_input, english_contract)
                .unwrap()
                .as_bytes(),
        );
        let same = compatibility(
            canonical_profile(&profile, &same_input, english_contract)
                .unwrap()
                .as_bytes(),
        );
        let changed = compatibility(
            canonical_profile(&profile, &changed_input, english_contract)
                .unwrap()
                .as_bytes(),
        );
        let mut french_profile = crate::config::ProfileConfig::default();
        french_profile.language.id = "fr".to_owned();
        let french = compatibility(
            canonical_profile(&french_profile, &first_input, french_contract)
                .unwrap()
                .as_bytes(),
        );
        assert_eq!(first, same);
        assert_ne!(first.profile_sha256, changed.profile_sha256);
        assert_ne!(first.session_sha256, changed.session_sha256);
        assert_ne!(first.profile_sha256, french.profile_sha256);
        assert_ne!(first.session_sha256, french.session_sha256);
    }
}
