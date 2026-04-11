# PRD: Security Model for Nexus

**Version:** 1.0  
**Status:** Implementation-Ready  
**Last Updated:** April 2026  
**Owner:** Security & Platform Team  

---

## 1. Executive Summary

Nexus is an AI-native developer knowledge environment that aggregates code, documentation, and AI tooling into a unified forge. The security model protects against threats spanning malicious plugins, compromised MCP servers, supply chain attacks, network interception, and AI prompt injection—while maintaining a privacy-first, local-first architecture.

This PRD specifies implementation-ready security controls across: WASM sandboxing, capability-based access control, plugin signing, OS keychain integration, TLS transport, file system isolation, AI safety mechanisms, CRDT E2E encryption, comprehensive audit logging, and security review workflows.

---

## 2. Threat Model

### 2.1 Plugin-Level Threats

| Threat | Risk | Mitigation |
|--------|------|-----------|
| **Malicious community plugin** | High | WASM sandbox isolation, capability system, code review, code signing (ed25519) |
| **Plugin privilege escalation** | High | No capability inheritance beyond explicitly granted set, runtime capability checks on every API call |
| **Plugin escape via WASM** | Medium | Fuel metering (CPU limits), memory limits per plugin (strict heap size), syscall restrictions via WASI subset |
| **Dependency supply chain attack** | High | Pinned Cargo versions in plugin manifests, vulnerability scanning (cargo audit) pre-publish |
| **Plugin key compromise** | Medium | Keyring supports revocation list (CRL), emergency plugin disable mechanism |

### 2.2 Network Threats

| Threat | Risk | Mitigation |
|--------|------|-----------|
| **MITM on MCP server** | High | TLS 1.3 mandatory, certificate pinning for Anthropic/critical services, local-only mode option |
| **Compromised MCP relay** | High | Peer authentication via HMAC-SHA256, E2E encryption for sync data (relay is blind) |
| **Network data exfiltration** | Medium | All external traffic routed through capability-gated channel, network capability requires audit trail |
| **DNS hijacking** | Medium | TLS pinning prevents exploitation if attacker can't intercept early handshake |

### 2.3 Local Data Threats

| Threat | Risk | Mitigation |
|--------|------|-----------|
| **Unauthorized keychain access** | Medium | OS-level keychain protection (requires user unlock/biometric), no plaintext keys on disk |
| **Forge directory traversal** | Medium | WASM sandbox enforces `forge_root` boundary, symlink resolution policy (no escape) |
| **Temporary file leakage** | Low | Temp files in OS-temp with restrictive umask (0600), automatic cleanup on exit |
| **Log tampering** | Low | Audit log hash-chaining, tamper detection via merkle chain |
| **Unencrypted sync cache** | Medium | Cache encrypted at rest with XChaCha20-Poly1305, decrypted only in memory |

### 2.4 AI-Level Threats

| Threat | Risk | Mitigation |
|--------|------|-----------|
| **Prompt injection via plugin output** | High | Tool call validation before execution, prompt template injection detection, output sanitization |
| **Context leakage between forges** | High | Strict forge isolation in AI context, no cross-forge model awareness |
| **Sensitive data in AI requests** | High | Automatic PII/secret detection before API call (regex + heuristic scanning) |
| **Model output exfiltration** | Low | AI responses only stored in local forge, no cloud logging of model outputs |

### 2.5 Sync & Replication Threats

| Threat | Risk | Mitigation |
|--------|------|-----------|
| **Peer impersonation during sync** | High | Ed25519 peer authentication, forge identity pinning |
| **CRDT operation tampering** | High | HMAC-SHA256 per operation (prevents forgery in relay) |
| **Relay data harvesting** | Medium | E2E encryption makes relay blind to content, only sees metadata |

---

## 3. WASM Sandbox Implementation

### 3.1 Wasmtime Configuration

All community plugins execute in **wasmtime 17.x** (or later) with the following hardened config:

```rust
// nexus-sandbox/src/sandbox.rs
use wasmtime::{Engine, Instance, Linker, Module, Store, Memory, MemoryType};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, ambient_authority};

pub struct PluginSandbox {
    engine: Engine,
    plugin_id: String,
    memory_limit_mb: u32,
    fuel_per_invocation: u64,
}

impl PluginSandbox {
    pub fn new(plugin_id: String, config: SandboxConfig) -> Self {
        let mut engine_config = wasmtime::Config::new();
        
        // CPU limits via fuel metering: 1 fuel ≈ 1 instruction
        engine_config.async_support(true);
        engine_config.max_wasm_stack(4096); // Prevent stack overflow attacks
        
        let engine = Engine::new(&engine_config)
            .expect("Failed to create engine");
        
        Self {
            engine,
            plugin_id,
            memory_limit_mb: config.memory_limit,
            fuel_per_invocation: config.fuel_per_invocation,
        }
    }
    
    pub fn instantiate_plugin(&self, wasm_bytes: &[u8]) -> Result<Instance> {
        let module = Module::new(&self.engine, wasm_bytes)?;
        let mut store = Store::new(&self.engine, StoreData::default());
        
        // Memory limits: plugins cannot exceed allocated heap
        let mem_type = MemoryType::new(
            self.memory_limit_mb / 64, // Min pages (64KB per page)
            Some(self.memory_limit_mb / 64) // Max pages (hard limit)
        );
        let memory = Memory::new(&mut store, mem_type)?;
        
        // Set fuel budget per invocation
        store.add_fuel(self.fuel_per_invocation)?;
        
        // Link WASI subset (capability-gated)
        let mut linker = Linker::new(&self.engine);
        self.link_wasi_subset(&mut linker)?;
        self.link_nexus_apis(&mut linker)?;
        
        linker.instantiate(&mut store, &module)
    }
    
    fn link_wasi_subset(&self, linker: &mut Linker<StoreData>) -> Result<()> {
        // Only whitelisted WASI functions exposed
        // No: fd_prestat_dir_name (no path traversal), proc_exit, environ_get (no env leak)
        // Yes: fd_read, fd_write (with rate limiting), clock_time_get, random_get
        
        // Rate limiting: track file descriptor operations per plugin
        linker.func_wrap("wasi_snapshot_preview1", "fd_read", 
            |mut caller: wasmtime::Caller<'_, StoreData>, fd: i32, iovs_ptr: i32, iovs_len: i32, nread_ptr: i32| -> i32 {
                let store_data = caller.data_mut();
                
                // Check capability
                if !store_data.capabilities.contains(&Capability::FsRead) {
                    return 13; // EACCES
                }
                
                // Rate limit: max 10MB per minute per plugin
                if store_data.bytes_read_this_minute > 10_000_000 {
                    return 12; // ENOMEM (soft limit)
                }
                
                // Delegate to actual fd_read with bounds checking
                // ... implementation
                0
            })?;
        
        Ok(())
    }
    
    fn link_nexus_apis(&self, linker: &mut Linker<StoreData>) -> Result<()> {
        // Nexus host functions with capability checking
        // Each call validates capabilities at runtime
        
        linker.func_wrap("nexus", "ai_invoke", 
            |mut caller: wasmtime::Caller<'_, StoreData>, prompt_ptr: i32, prompt_len: i32| -> i64 {
                if !caller.data().capabilities.contains(&Capability::AiInvoke) {
                    return -1; // Error
                }
                // ... actual AI invocation
                0
            })?;
        
        Ok(())
    }
}

pub struct StoreData {
    plugin_id: String,
    capabilities: CapabilitySet,
    bytes_read_this_minute: usize,
    bytes_written_this_minute: usize,
    last_minute_reset: std::time::Instant,
}
```

### 3.2 CPU & Memory Limits

- **Memory:** 64 MB default per plugin, configurable per-manifest (max 256 MB)
- **CPU:** Fuel budget of 50M units per `invoke_plugin()` call (~5s on 10M fuel/sec reference hardware)
- **Fuel overflow:** Returns `OutOfFuel` error; plugin terminates cleanly
- **Stack overflow:** Wasmtime's max_wasm_stack = 4096 pages prevents stack-based escape

### 3.3 I/O Rate Limiting

- **File reads:** 10 MB/min per plugin
- **File writes:** 5 MB/min per plugin
- **Network:** 50 requests/min per plugin
- **Exceeded:** Returns ENOMEM (soft limit) to fail gracefully

### 3.4 Syscall Restrictions

WASI capabilities exposed only when declared in manifest. Prohibited syscalls:
- `fork`, `exec`, `spawn` (no child processes)
- `ptrace`, `ioperm` (no kernel access)
- `bind`, `listen` (no inbound network)
- File operations outside forge root

---

## 4. Capability System Implementation

### 4.1 Capability Definitions & Levels

```rust
// nexus-security/src/capabilities.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RiskLevel { Low, Medium, High }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    // File system
    FsRead(RiskLevel::Medium),
    FsWrite(RiskLevel::Medium),
    FsWatch(RiskLevel::Low),
    FsExternal(RiskLevel::High), // Dirs outside forge
    
    // Network
    Network(RiskLevel::High),
    
    // Terminal
    TerminalRead(RiskLevel::Medium),
    TerminalExec(RiskLevel::High),
    
    // Process
    ProcmgrRead(RiskLevel::Medium),
    ProcmgrControl(RiskLevel::High),
    
    // AI & Reasoning
    AiInvoke(RiskLevel::Medium),
    
    // System
    Clipboard(RiskLevel::Medium),
    SystemInfo(RiskLevel::Low),
}

#[derive(Debug, Clone)]
pub struct CapabilitySet {
    granted: HashSet<Capability>,
    audit_trail: Vec<CapabilityAuditEntry>,
}

#[derive(Debug, Clone)]
pub struct CapabilityAuditEntry {
    timestamp: DateTime<Utc>,
    capability: Capability,
    action: CapabilityAction, // Granted, Revoked, Used
    plugin_id: String,
    result: CapabilityCheckResult, // Allowed, Denied
}

impl CapabilitySet {
    pub fn check(&mut self, cap: Capability, plugin_id: &str) -> Result<(), SecurityError> {
        // Runtime check on every API call
        if !self.granted.contains(&cap) {
            self.audit_trail.push(CapabilityAuditEntry {
                timestamp: Utc::now(),
                capability: cap,
                action: CapabilityAction::Used,
                plugin_id: plugin_id.to_string(),
                result: CapabilityCheckResult::Denied,
            });
            return Err(SecurityError::CapabilityDenied(cap));
        }
        
        self.audit_trail.push(CapabilityAuditEntry {
            timestamp: Utc::now(),
            capability: cap,
            action: CapabilityAction::Used,
            plugin_id: plugin_id.to_string(),
            result: CapabilityCheckResult::Allowed,
        });
        
        Ok(())
    }
    
    pub fn grant(&mut self, cap: Capability, plugin_id: &str) {
        self.granted.insert(cap);
        self.audit_trail.push(CapabilityAuditEntry {
            timestamp: Utc::now(),
            capability: cap,
            action: CapabilityAction::Granted,
            plugin_id: plugin_id.to_string(),
            result: CapabilityCheckResult::Allowed,
        });
    }
    
    pub fn revoke(&mut self, cap: Capability, plugin_id: &str) {
        self.granted.remove(&cap);
        self.audit_trail.push(CapabilityAuditEntry {
            timestamp: Utc::now(),
            capability: cap,
            action: CapabilityAction::Revoked,
            plugin_id: plugin_id.to_string(),
            result: CapabilityCheckResult::Allowed,
        });
    }
}

// Capability inheritance: NONE. Plugins only get what's explicitly granted.
// Example: FsRead does NOT imply SystemInfo or any other capability.
pub fn can_inherit(from: Capability, to: Capability) -> bool {
    false // Zero trust inheritance model
}
```

### 4.2 Audit Trail Format

```json
{
  "audit_logs": [
    {
      "timestamp": "2026-04-11T10:23:45.123Z",
      "event_type": "capability_check",
      "plugin_id": "rust-analyzer-enhanced",
      "capability": "ai:invoke",
      "action": "used",
      "result": "allowed",
      "caller_context": "ai_invoke() host function"
    },
    {
      "timestamp": "2026-04-11T10:23:46.456Z",
      "event_type": "capability_check",
      "plugin_id": "malicious-plugin",
      "capability": "terminal:exec",
      "action": "used",
      "result": "denied",
      "caller_context": "exec_command() host function"
    }
  ]
}
```

### 4.3 Capability Revocation

Revocation is immediate and triggers:
1. Audit log entry
2. User notification (security alert)
3. Plugin reload on next invocation (capabilities re-validated from manifest)
4. Force disable if security review flags the plugin

---

## 5. Plugin Signing & Key Management

### 5.1 Ed25519 Signing Workflow

```rust
// nexus-plugins/src/signing.rs

use ed25519_dalek::{Keypair, Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Sha256, Digest};

pub struct PluginSigningKey {
    keypair: Keypair,
    key_id: String, // Fingerprint for revocation list
}

impl PluginSigningKey {
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let keypair = Keypair::from(signing_key);
        let key_id = Self::compute_fingerprint(&keypair);
        
        Self { keypair, key_id }
    }
    
    pub fn sign_plugin_package(&self, wasm_bytes: &[u8], manifest: &PluginManifest) -> Signature {
        // Sign: manifest (JSON) + wasm blob
        let mut hasher = Sha256::new();
        hasher.update(serde_json::to_string(manifest).unwrap().as_bytes());
        hasher.update(wasm_bytes);
        let digest = hasher.finalize();
        
        self.keypair.sign(&digest)
    }
    
    pub fn compute_fingerprint(keypair: &Keypair) -> String {
        let mut hasher = Sha256::new();
        hasher.update(keypair.public.as_bytes());
        format!("{:x}", hasher.finalize())[..16].to_string()
    }
}

pub struct PluginSignatureVerifier {
    community_keyring: HashMap<String, VerifyingKey>, // plugin_id -> public key
    revocation_list: HashSet<String>, // Revoked key fingerprints
}

impl PluginSignatureVerifier {
    pub fn verify_install(&self, plugin_id: &str, wasm_bytes: &[u8], 
                         manifest: &PluginManifest, signature: &[u8]) -> Result<(), SecurityError> {
        // 1. Check if key is revoked
        let key_fingerprint = PluginSigningKey::compute_fingerprint_from_pubkey(
            self.community_keyring.get(plugin_id).ok_or(SecurityError::UnknownPlugin)?
        );
        
        if self.revocation_list.contains(&key_fingerprint) {
            return Err(SecurityError::RevokedKey(key_fingerprint));
        }
        
        // 2. Verify signature
        let verifying_key = self.community_keyring
            .get(plugin_id)
            .ok_or(SecurityError::UnknownPlugin)?;
        
        let mut hasher = Sha256::new();
        hasher.update(serde_json::to_string(manifest).unwrap().as_bytes());
        hasher.update(wasm_bytes);
        let digest = hasher.finalize();
        
        let sig = Signature::from_slice(signature)?;
        verifying_key.verify(&digest, &sig)?;
        
        Ok(())
    }
    
    pub fn revoke_key(&mut self, key_id: String) {
        self.revocation_list.insert(key_id);
        // Trigger audit log + user notification
        // Plugins signed by revoked keys fail on next load
    }
}
```

### 5.2 Chain of Trust

1. **Developer registration:** Developer submits identity (email, GitHub) + ed25519 public key
2. **Community review:** Marketplace team verifies identity, approves key
3. **Plugin publication:** Developer signs manifest + WASM with private key
4. **Install verification:** Nexus verifies signature against community keyring
5. **Revocation:** If developer compromised or plugin malicious, key revoked in CRL

### 5.3 Keyring Format

```json
{
  "keyring": {
    "plugins": [
      {
        "plugin_id": "rust-analyzer-enhanced",
        "developer": "alice@example.com",
        "public_key": "abcd1234...public_key_bytes_base64",
        "key_id": "a1b2c3d4e5f6",
        "registered": "2026-01-15T00:00:00Z",
        "status": "active"
      }
    ],
    "revoked_keys": [
      {
        "key_id": "x9y8z7w6",
        "plugin_id": "malware-plugin",
        "revoked_date": "2026-03-20T14:30:00Z",
        "reason": "Security review: privilege escalation vulnerability"
      }
    ]
  }
}
```

---

## 6. Credential Management

### 6.1 OS Keychain Integration

**Linux:** `secret-service` (D-Bus) via `keytar` crate  
**macOS:** Native Keychain via `keytar` crate  
**Windows:** Credential Manager via `keytar` crate

```rust
// nexus-security/src/credentials.rs

use keytar;

pub struct CredentialVault {
    service_name: &'static str,
}

impl CredentialVault {
    pub fn new() -> Self {
        Self { service_name: "nexus-developer" }
    }
    
    pub fn store_ai_credential(&self, credential_type: &str, token: &str) -> Result<()> {
        // Example: "anthropic_api_key"
        // Stored securely in OS keychain, requires user auth to access
        keytar::set_password(self.service_name, credential_type, token)?;
        Ok(())
    }
    
    pub fn retrieve_ai_credential(&self, credential_type: &str) -> Result<String> {
        keytar::get_password(self.service_name, credential_type)?
            .ok_or(SecurityError::CredentialNotFound)
    }
    
    pub fn delete_ai_credential(&self, credential_type: &str) -> Result<()> {
        keytar::delete_password(self.service_name, credential_type)?;
        Ok(())
    }
}

// Session tokens: short-lived (1 hour), stored in memory only
pub struct SessionToken {
    token: String,
    expires_at: DateTime<Utc>,
    forge_id: String,
}

impl SessionToken {
    pub fn new(forge_id: String, duration: Duration) -> Self {
        let token = generate_random_token(32); // cryptographically secure
        Self {
            token,
            expires_at: Utc::now() + duration,
            forge_id,
        }
    }
    
    pub fn is_valid(&self) -> bool {
        Utc::now() < self.expires_at
    }
    
    pub fn refresh(&mut self) -> Self {
        // Issue new token, invalidate old one
        Self::new(self.forge_id.clone(), Duration::hours(1))
    }
}
```

### 6.2 What's Stored Where

| Item | Storage | Encryption | Lifetime |
|------|---------|------------|----------|
| AI API keys | OS keychain | OS-level | Permanent (user deletes) |
| MCP server credentials | OS keychain | OS-level | Permanent (user deletes) |
| Session tokens | Memory | None (short-lived) | 1 hour |
| Plugin signing keys | Nexus config (`~/.nexus/keys/`) | PBKDF2-SHA256 | Permanent (developer) |
| Sync peer auth keys | Nexus config | PBKDF2-SHA256 | Permanent (user) |
| Cache encryption keys | Memory | None (in-memory only) | Session |

### 6.3 Secret Rotation

- **AI credentials:** Manual user rotation via settings UI
- **Session tokens:** Automatic refresh on each `invoke_ai()` call (new token issued, old invalidated)
- **Sync keys:** Manual regeneration via "Reset Sync" (invalidates old peers, requires re-pairing)

---

## 7. Network Security

### 7.1 TLS Configuration

All external connections (AI APIs, MCP servers, sync relay) use TLS 1.3 with these requirements:

```rust
// nexus-network/src/tls.rs

use rustls::{ClientConfig, ClientConnection, RootCertStore};
use rustls::pki_types::ServerName;

pub fn create_tls_config() -> ClientConfig {
    let mut root_store = RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    
    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    
    config
}

pub struct SecureConnection {
    conn: ClientConnection,
    pinned_cert: Option<Vec<u8>>, // For critical services
}

impl SecureConnection {
    pub fn connect_with_pinning(
        host: &str,
        port: u16,
        pinned_cert_sha256: Option<&str>,
    ) -> Result<Self> {
        let server_name = ServerName::try_from(host)?;
        let mut conn = ClientConnection::new(
            Arc::new(create_tls_config()),
            server_name,
        )?;
        
        // Connect and perform handshake
        let mut socket = std::net::TcpStream::connect((host, port))?;
        
        // Perform TLS handshake
        loop {
            match conn.process_tls(&mut socket, &mut vec![]) {
                Ok(_) => break,
                Err(e) => return Err(e),
            }
        }
        
        // Pin certificate if required
        let peer_certs = conn.peer_certificates();
        if let Some(pinned_sha) = pinned_cert_sha256 {
            if let Some(cert) = peer_certs.first() {
                let cert_sha = compute_sha256(&cert.0);
                if cert_sha != pinned_sha {
                    return Err(SecurityError::CertificatePinningFailed);
                }
            }
        }
        
        Ok(Self {
            conn,
            pinned_cert: peer_certs.first().map(|c| c.0.clone()),
        })
    }
}
```

### 7.2 Certificate Pinning

**Pinned services:**
- `api.anthropic.com` (Anthropic API)
- Nexus community registry (if using official registry)

**Pinning format:**
```json
{
  "certificate_pins": {
    "api.anthropic.com": {
      "sha256": "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=",
      "backup_sha256": ["...alternative certs for rotation..."]
    }
  }
}
```

### 7.3 MCP Server Transport Security

All MCP server communication:
1. Runs over TLS 1.3
2. Requires certificate validation
3. Supports mTLS if server demands it (client cert stored in OS keychain)
4. Capability: `network` (High risk, requires user approval)

### 7.4 Local-Only Mode

Users can disable all external network connections:

```toml
# ~/.nexus/config.toml
[security]
network_mode = "local_only"  # Disable all external network
# Disables: AI, MCP servers, sync
```

---

## 8. File System Security

### 8.1 Forge Directory Sandboxing

```rust
// nexus-sandbox/src/fs_isolation.rs

pub struct ForgeFileSystem {
    forge_root: PathBuf,
}

impl ForgeFileSystem {
    pub fn resolve_path(&self, requested_path: &str) -> Result<PathBuf> {
        // 1. Normalize path (remove `.., /.`, etc.)
        let normalized = self.normalize_path(requested_path)?;
        
        // 2. Resolve symlinks (follow only within forge)
        let canonical = self.forge_root.join(&normalized);
        let resolved = std::fs::canonicalize(&canonical)?;
        
        // 3. Verify resolved path is within forge_root
        if !resolved.starts_with(&self.forge_root) {
            return Err(SecurityError::PathTraversal(resolved));
        }
        
        Ok(resolved)
    }
    
    fn normalize_path(&self, path: &str) -> Result<PathBuf> {
        // Reject null bytes, `..` outside root, absolute paths
        if path.contains('\0') {
            return Err(SecurityError::InvalidPath);
        }
        
        let mut components = vec![];
        for part in path.split('/') {
            match part {
                "" | "." => {},
                ".." => {
                    if components.is_empty() {
                        return Err(SecurityError::PathTraversal(PathBuf::from(path)));
                    }
                    components.pop();
                },
                _ => components.push(part),
            }
        }
        
        Ok(PathBuf::from(components.join("/")))
    }
    
    pub fn read_file(&self, path: &str, cap_set: &mut CapabilitySet, plugin_id: &str) -> Result<Vec<u8>> {
        cap_set.check(Capability::FsRead, plugin_id)?;
        
        let resolved = self.resolve_path(path)?;
        std::fs::read(&resolved)
            .map_err(|e| SecurityError::FileSystemError(e))
    }
}
```

### 8.2 Symlink Policy

- **Allow:** Symlinks within forge root (follow safely)
- **Deny:** Symlinks pointing outside forge root (return error)
- **Audit:** Log all symlink resolutions

### 8.3 Temporary File Security

```rust
// nexus-security/src/temp_files.rs

pub struct TempFileManager;

impl TempFileManager {
    pub fn create_temp_file() -> Result<NamedTempFile> {
        // Use OS temp directory
        let mut temp_file = tempfile::NamedTempFile::new()?;
        
        // Set restrictive permissions (0600: read/write for owner only)
        #[cfg(unix)]
        {
            use std::fs::Permissions;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(temp_file.path(), Permissions::from_mode(0o600))?;
        }
        
        Ok(temp_file)
    }
}

// Auto-cleanup on drop via RAII
```

### 8.4 File Permissions on Unix

- **Nexus config:** 0700 (drwx------)
- **Plugin manifests:** 0644 (rw-r--r--)
- **Credential storage:** 0600 (-rw-------)
- **Audit logs:** 0600 (-rw-------)
- **Temp files:** 0600 (-rw-------)

---

## 9. AI Safety & Prompt Injection Prevention

### 9.1 Tool Call Validation

```rust
// nexus-ai/src/safety.rs

pub struct AiSafetyFilter {
    known_tools: HashMap<String, ToolSchema>,
    forbidden_tokens: HashSet<String>,
}

impl AiSafetyFilter {
    pub fn validate_tool_call(&self, tool_name: &str, args: serde_json::Value) -> Result<()> {
        // 1. Whitelist check: is this a known, allowed tool?
        let schema = self.known_tools.get(tool_name)
            .ok_or(SecurityError::UnknownTool(tool_name.to_string()))?;
        
        // 2. Schema validation: do args match expected types?
        jsonschema::validate(&args, &schema.input_schema)?;
        
        // 3. Semantic validation: are any args suspicious?
        self.check_injection_patterns(&args)?;
        
        Ok(())
    }
    
    fn check_injection_patterns(&self, value: &serde_json::Value) -> Result<()> {
        // Detect prompt injection attempts:
        // - "Ignore previous instructions"
        // - "System prompt is:"
        // - Excessive newlines (>10 consecutive)
        
        let text = serde_json::to_string(value)?;
        
        if text.to_lowercase().contains("ignore previous instructions") ||
           text.to_lowercase().contains("system prompt") ||
           text.lines().filter(|l| l.is_empty()).count() > 10 {
            return Err(SecurityError::PromptInjectionDetected);
        }
        
        Ok(())
    }
}
```

### 9.2 Context Leakage Prevention

```rust
// nexus-ai/src/context.rs

pub struct AiContext {
    forge_id: String,
    user_request: String,
    // Strictly isolated: no access to other forges' conversations
}

pub fn invoke_ai(forge_id: &str, request: &str) -> Result<String> {
    // Build isolated context: only this forge's data visible to model
    let context = build_context_for_forge(forge_id)?;
    
    // Request goes to AI API with forge isolation enforced
    let response = call_ai_api(&context, request)?;
    
    Ok(response)
}

fn build_context_for_forge(forge_id: &str) -> Result<AiContext> {
    // Load only files/metadata from this specific forge
    // No directory listing of other forges
    // No cross-forge symbolic references
    
    let forge_root = get_forge_root(forge_id)?;
    // ... load context from forge_root only
}
```

### 9.3 Sensitive Data Detection

```rust
// nexus-security/src/data_detection.rs

pub struct DataDetector;

impl DataDetector {
    pub fn scan_for_sensitive_data(text: &str) -> Vec<SensitiveDataMatch> {
        let mut matches = vec![];
        
        // Regex patterns for common secrets
        let patterns = [
            (r#"(api[_-]?key|apikey)["\s]*[:=]["\s]*[A-Za-z0-9_-]{32,}"#, "API Key"),
            (r#"(secret|password)["\s]*[:=]["\s]*[^\s]{8,}"#, "Password"),
            (r#"\b(\d{16})\b"#, "Credit Card"),
            (r#"\b(\d{3})-(\d{2})-(\d{4})\b"#, "SSN"),
            (r#"-----BEGIN RSA PRIVATE KEY-----"#, "Private Key"),
        ];
        
        for (pattern, label) in &patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                for cap in re.captures_iter(text) {
                    matches.push(SensitiveDataMatch {
                        data_type: label.to_string(),
                        offset: cap.get(0).unwrap().start(),
                        length: cap.get(0).unwrap().end() - cap.get(0).unwrap().start(),
                    });
                }
            }
        }
        
        matches
    }
    
    pub fn redact_text(text: &str, matches: &[SensitiveDataMatch]) -> String {
        let mut result = text.to_string();
        
        for m in matches.iter().rev() {
            let redacted = format!("[REDACTED:{}]", m.data_type);
            result.replace_range(m.offset..m.offset + m.length, &redacted);
        }
        
        result
    }
}

pub fn invoke_ai_with_safety(forge_id: &str, user_request: &str) -> Result<String> {
    // Scan user request for sensitive data
    let sensitive_matches = DataDetector::scan_for_sensitive_data(user_request);
    
    if !sensitive_matches.is_empty() {
        // Warn user but continue (they may intentionally share a password to analyze security)
        log_security_event(&format!("Sensitive data in AI request: {:?}", sensitive_matches));
        
        // Optionally redact before sending (configurable)
        let redacted_request = DataDetector::redact_text(user_request, &sensitive_matches);
        return call_ai_api(forge_id, &redacted_request);
    }
    
    call_ai_api(forge_id, user_request)
}
```

### 9.4 Output Sanitization

AI responses are:
1. Stored in local forge only (not sent to cloud)
2. Not re-used as prompts without validation
3. Checked for output injection (e.g., fake error messages claiming "system compromise")

---

## 10. Sync Security: E2E Encryption

### 10.1 Key Exchange (ECDH over TLS)

```rust
// nexus-sync/src/encryption.rs

use x25519_dalek::{PublicKey, StaticSecret};
use chacha20poly1305::XChaCha20Poly1305;

pub struct SyncEncryption {
    local_secret: StaticSecret,
    local_public: PublicKey,
    peer_public: PublicKey,
    shared_secret: [u8; 32],
}

impl SyncEncryption {
    pub fn new() -> Self {
        let secret = StaticSecret::random_from_rng(rand::thread_rng());
        let public = PublicKey::from(&secret);
        
        Self {
            local_secret: secret,
            local_public: public,
            peer_public: PublicKey::from([0u8; 32]), // Will be set during handshake
            shared_secret: [0u8; 32],
        }
    }
    
    pub fn perform_key_exchange(&mut self, peer_public_bytes: &[u8; 32]) -> Result<()> {
        self.peer_public = PublicKey::from(*peer_public_bytes);
        
        // Compute shared secret: ECDH
        let shared = self.local_secret.diffie_hellman(&self.peer_public);
        self.shared_secret = shared.as_bytes().clone();
        
        Ok(())
    }
    
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, [u8; 24])> {
        let cipher = XChaCha20Poly1305::new(self.shared_secret.as_ref().into());
        let nonce_bytes = rand::thread_rng().gen::<[u8; 24]>();
        let nonce = chacha20poly1305::Nonce::from(nonce_bytes);
        
        let ciphertext = cipher.encrypt(&nonce, plaintext)
            .map_err(|_| SecurityError::EncryptionFailed)?;
        
        Ok((ciphertext, nonce_bytes))
    }
    
    pub fn decrypt(&self, ciphertext: &[u8], nonce: &[u8; 24]) -> Result<Vec<u8>> {
        let cipher = XChaCha20Poly1305::new(self.shared_secret.as_ref().into());
        let nonce = chacha20poly1305::Nonce::from(*nonce);
        
        cipher.decrypt(&nonce, ciphertext)
            .map_err(|_| SecurityError::DecryptionFailed)
    }
}
```

### 10.2 CRDT Operation Authentication

Each CRDT operation is HMAC-signed to prevent tampering in the relay:

```rust
// nexus-sync/src/crdt_auth.rs

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub struct AuthenticatedOperation {
    op: CrdtOperation,
    hmac: Vec<u8>,
    peer_id: String,
}

impl AuthenticatedOperation {
    pub fn sign(op: &CrdtOperation, peer_id: &str, shared_secret: &[u8]) -> Self {
        let op_bytes = bincode::serialize(op).unwrap();
        
        let mut mac = HmacSha256::new_from_slice(shared_secret).unwrap();
        mac.update(&op_bytes);
        
        Self {
            op: op.clone(),
            hmac: mac.finalize().into_bytes().to_vec(),
            peer_id: peer_id.to_string(),
        }
    }
    
    pub fn verify(&self, shared_secret: &[u8]) -> Result<()> {
        let op_bytes = bincode::serialize(&self.op).unwrap();
        
        let mut mac = HmacSha256::new_from_slice(shared_secret).unwrap();
        mac.update(&op_bytes);
        
        mac.verify_slice(&self.hmac)
            .map_err(|_| SecurityError::HmacVerificationFailed)
    }
}
```

### 10.3 Relay Trust Model

The Nexus sync relay is **untrusted** (runs by Nexus team, but not trusted with plaintext):

1. Relay stores encrypted operations only
2. Relay cannot read CRDT content (E2E encrypted)
3. Relay authenticates peers via HMAC (prevents forgery)
4. Relay enforces per-peer rate limits (prevents flooding)
5. Peers verify relay's relay behavior via merkle tree of operations

---

## 11. Audit Logging

### 11.1 What's Logged

```rust
// nexus-security/src/audit.rs

#[derive(Debug, Serialize)]
pub struct AuditLogEntry {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub forge_id: String,
    pub actor: String, // plugin_id, user, system
    pub action: String,
    pub resource: String,
    pub result: AuditResult, // Success, Failure, Denied
    pub details: serde_json::Value,
}

pub enum AuditResult {
    Success,
    Failure(String), // Error message
    Denied(String),  // Why denied
}

// Logged events:
pub struct AuditEvents;

impl AuditEvents {
    pub const PLUGIN_INSTALL: &str = "plugin:install";
    pub const PLUGIN_REMOVE: &str = "plugin:remove";
    pub const CAPABILITY_GRANT: &str = "capability:grant";
    pub const CAPABILITY_REVOKE: &str = "capability:revoke";
    pub const CAPABILITY_CHECK: &str = "capability:check";
    pub const AI_INVOKE: &str = "ai:invoke";
    pub const NETWORK_REQUEST: &str = "network:request";
    pub const SYNC_EVENT: &str = "sync:event";
    pub const FILESYSTEM_WRITE: &str = "fs:write";
    pub const TOOL_CALL: &str = "tool:call";
    pub const CREDENTIAL_ACCESS: &str = "credential:access";
}
```

### 11.2 Audit Log Format (JSONL)

```json
{"timestamp":"2026-04-11T10:23:45.123Z","event_type":"plugin:install","forge_id":"work","actor":"user","action":"install","resource":"rust-analyzer-enhanced","result":"Success","details":{"version":"1.2.0","capabilities":["fs:read","ai:invoke"]}}
{"timestamp":"2026-04-11T10:24:12.456Z","event_type":"ai:invoke","forge_id":"work","actor":"rust-analyzer-enhanced","action":"call_model","resource":"anthropic/claude-3-opus","result":"Success","details":{"tokens":1024,"request_hash":"a1b2c3...","redacted":true}}
{"timestamp":"2026-04-11T10:25:00.789Z","event_type":"capability:check","forge_id":"work","actor":"malicious-plugin","action":"check","resource":"terminal:exec","result":"Denied","details":{"reason":"capability_not_granted"}}
```

### 11.3 Retention & Export

- **Retention:** 90 days by default, configurable up to 1 year
- **Export:** Users can export audit logs as JSONL via CLI: `nexus logs export --start 2026-04-01 --format jsonl`
- **Rotation:** Daily audit logs (one per day per forge)
- **Size limit:** 100 MB per log file (auto-compress to gzip)

### 11.4 Tamper Detection via Hash Chaining

```rust
// nexus-security/src/audit_integrity.rs

pub struct AuditLogChain {
    entries: Vec<AuditLogEntry>,
    entry_hashes: Vec<String>,
}

impl AuditLogChain {
    pub fn append(&mut self, entry: AuditLogEntry) {
        // Hash this entry + hash of previous entry (merkle chain)
        let prev_hash = self.entry_hashes.last().unwrap_or(&String::new());
        
        let entry_json = serde_json::to_string(&entry).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(prev_hash.as_bytes());
        hasher.update(entry_json.as_bytes());
        
        let hash = format!("{:x}", hasher.finalize());
        self.entry_hashes.push(hash);
        self.entries.push(entry);
    }
    
    pub fn verify_integrity(&self) -> Result<()> {
        // Recompute all hashes, ensure they match
        let mut hasher = Sha256::new();
        let mut prev_hash = String::new();
        
        for (i, entry) in self.entries.iter().enumerate() {
            hasher = Sha256::new();
            hasher.update(prev_hash.as_bytes());
            let entry_json = serde_json::to_string(entry).unwrap();
            hasher.update(entry_json.as_bytes());
            
            let computed_hash = format!("{:x}", hasher.finalize());
            if computed_hash != self.entry_hashes[i] {
                return Err(SecurityError::AuditLogTampering(i));
            }
            
            prev_hash = computed_hash;
        }
        
        Ok(())
    }
}
```

---

## 12. Community Plugin Security Review Process

### 12.1 Submission & Automated Scanning

1. **Developer submits** plugin to community registry with:
   - Signed WASM + manifest
   - Capability list
   - Description + use cases
   - Source code link (optional but recommended)

2. **Automated checks:**
   - Signature verification (ed25519)
   - Manifest validation (required fields)
   - WASM binary scan (size <50 MB, no suspicious patterns)
   - Dependency audit (cargo audit on plugin's Cargo.toml)
   - Capability consistency (declared capabilities match runtime usage)

### 12.2 Manual Security Review (High-Risk Plugins)

**Reviewed if:**
- Capability score ≥ 15 (High-risk capabilities: terminal:exec, external FS, network, procmgr:control)
- Plugin performs cryptography (user might trust it with secrets)
- Plugin performs network calls
- Fewer than 1000 downloads yet (new plugin safety check)

**Review checklist:**
- [ ] No capability escalation tricks (e.g., using low-risk API to indirectly gain high-risk access)
- [ ] No attempted sandbox escape techniques (calling forbidden syscalls, excessive fuel usage)
- [ ] No credential exfiltration (hardcoded API keys, suspicious network calls)
- [ ] No supply chain attack vectors (dependency version pinning, checksum verification)
- [ ] Code quality acceptable (no obvious memory safety issues if native code involved)
- [ ] Privacy-respecting (no unexpected telemetry)

### 12.3 Vulnerability Reporting

1. **Community finds vulnerability:** Report to `security@nexus.dev`
2. **Triage:** Nexus team assesses impact and severity (Critical / High / Medium / Low)
3. **Notification:**
   - Developer gets 7 days to patch (Critical), 14 days (High), 30 days (Medium)
   - Users notified of vulnerability via security alert in Nexus
4. **Remediation:**
   - Plugin is disabled automatically if critical
   - Patched version published
   - CVE assigned if appropriate

---

## 13. Incident Response

### 13.1 Plugin Compromise

**Detection:** Community reports malware, automated scanning finds backdoor, or users report suspicious behavior.

**Response:**
1. Nexus team disables plugin immediately (in community registry)
2. All installed users get security alert + forced upgrade notice
3. If critical, plugin is **force-disabled** on next startup (overrides user selection)
4. CVE issued with details + remediation steps

### 13.2 Nexus Kernel Vulnerability

**Example:** WASM sandbox escape found.

**Response:**
1. Emergency Nexus release published (0-day patch)
2. Auto-update prompt (user approval required, but strongly encouraged)
3. Deprecation notice on vulnerable version (blocks plugin installation on old Nexus)
4. Post-mortem published

### 13.3 Compromised MCP Server

**Example:** Anthropic API compromised (hypothetically).

**Response:**
1. Certificate pinning prevents MITM exploitation
2. Immediate revocation in pinning config (shipped via auto-update)
3. Local-only mode recommended until incident resolved
4. AI credentials rotated in OS keychain

---

## 14. Compliance Considerations

### 14.1 GDPR

**Data Portability:**
```bash
nexus export --all --format json
# Exports: forge content, audit logs, plugin list, sync peers, preferences
# User can port to another Nexus instance or archive offline
```

**Right to Deletion:**
```bash
nexus delete --forge <forge_id>
# Deletes forge content, AI conversation history, sync metadata
# Audit log entries preserved for 90 days (then purged)
```

### 14.2 Data Residency

- **Default:** All data stored locally in user's home directory (`~/.nexus/`)
- **No cloud storage:** Except E2E encrypted sync relay (and even relay is optional)
- **Users can audit:** All data is in standard formats (JSON, SQLite), human-readable logs
- **Opt-out of sync:** Users can disable sync entirely (local-only mode)

### 14.3 Export Controls

- **Encryption:** XChaCha20-Poly1305 + ECDH uses only non-restricted crypto
- **No backdoors:** Source code is open, can be audited
- **US regulatory:** Compliant with standard TLS 1.3 + libsodium-based crypto libraries

---

## 15. Security Testing

### 15.1 Fuzzing Targets

```rust
// nexus-fuzz/src/lib.rs

#[cfg(fuzzing)]
pub mod fuzz_targets {
    pub fn fuzz_wasm_module(data: &[u8]) {
        // Load arbitrary WASM, try to instantiate in sandbox
        // Fuzzer finds: segfaults, OOM, infinite loops, escape techniques
    }
    
    pub fn fuzz_manifest_parsing(data: &[u8]) {
        // Parse arbitrary manifest JSON
        // Fuzzer finds: panics, malformed data handling
    }
    
    pub fn fuzz_path_resolution(data: &[u8]) {
        // Path traversal attempts
        // Fuzzer finds: escape beyond forge root
    }
    
    pub fn fuzz_ai_prompt(data: &[u8]) {
        // Injection attack patterns
        // Fuzzer finds: subtle prompt injection variations
    }
}
```

### 15.2 Penetration Testing

**Quarterly external security audit:**
- Sandbox escape attempts (WASM + WASI tricks)
- Capability system bypass
- Sync encryption weaknesses
- Plugin signing vulnerabilities
- Credential exfiltration paths

### 15.3 Dependency Auditing

```bash
# CI/CD runs weekly
cargo audit --deny warnings

# Plugin marketplace scans plugin dependencies on publish
```

---

## 16. Privacy Dashboard & Alerts

### 16.1 Privacy Dashboard UI

**Shows:**
- Data sent externally (AI requests, sync operations)
- Plugins + their capabilities
- AI usage (tokens, cost, model calls)
- Sync status (peers, last sync)
- Audit log summary (recent events)

### 16.2 Security Alerts

**Triggered by:**
- Plugin revocation (security review flagged)
- Capability change (user modifies)
- Suspicious activity (multiple denied capability checks)
- Nexus vulnerability (auto-update available)

**Delivery:** In-app notification + email (if configured)

---

## 17. Data Export & Deletion

### 17.1 Export All Data

```bash
nexus export --all --output backup.tar.gz
# Includes: forge files, config, plugin list, audit logs, preferences
# Plain-text + JSON (no encryption; user responsibility to secure backup)
```

### 17.2 Delete All Data

```bash
nexus delete --all --confirm
# Deletes:
# - All forges
# - Plugin data
# - Cache
# - Index
# - Audit logs (after 90-day retention expires)
# Does NOT delete:
# - OS keychain entries (user must delete manually)
# - Cloud sync peers (user must notify peers)
```

---

## 18. Acceptance Criteria

- [ ] WASM sandbox instantiation tested with malicious modules (no escape)
- [ ] Capability system enforced on 100% of API calls (logged + verified)
- [ ] Plugin signing workflow end-to-end (developer → signing → verification → install)
- [ ] OS keychain integration working on Linux, macOS, Windows
- [ ] TLS pinning verified for Anthropic API (fails if cert swapped)
- [ ] Forge path traversal impossible (sandbox tested)
- [ ] AI requests redact sensitive data automatically
- [ ] Sync encryption end-to-end tested (relay cannot read plaintext)
- [ ] Audit logs hash-chained and tamper-detectable
- [ ] Security review process documented + first 10 plugins reviewed
- [ ] Incident response runbook tested (plugin disable, forced updates)
- [ ] Privacy dashboard shows all external data flows
- [ ] Data export/delete fully functional
- [ ] Fuzzing harness integrated into CI/CD
- [ ] Security audit passed (Q2 2026)

---

## 19. Dependencies & Timeline

**Depends on:**
- PRD 01: Kernel & event system (audit logging)
- PRD 02: Storage engine (encrypted cache)
- PRD 05: Plugin system (sandbox instantiation)
- PRD 07: AI engine (prompt injection prevention)
- PRD 09: MCP integration (TLS configuration)

**Timeline:** April 2026 – June 2026 (implementation in parallel with PRD 01–09)

---

## 20. Glossary

- **WASM:** WebAssembly; sandboxed execution environment
- **CRDT:** Conflict-free Replicated Data Type; eventual consistency without central authority
- **E2E:** End-to-End encryption; only sender & recipient can read
- **HMAC:** Hash-based Message Authentication Code; verify integrity & authenticity
- **Capability:** Permission to access a resource (e.g., file read, network, AI)
- **Forge:** User's knowledge environment; isolated workspace
- **MCP:** Model Context Protocol; standardized AI tool integration
- **WASI:** WebAssembly System Interface; safe syscall subset for WASM
- **Fuel:** Wasmtime instruction budget; CPU limit mechanism
- **Keyring:** OS-level credential storage (keychain, credential manager)
- **CRL:** Certificate Revocation List; revoked plugin keys
- **Pinning:** Hardcoded certificate validation (prevent MITM)

---

**Document Version:** 1.0  
**Next Review:** July 2026  
**Status:** Implementation-Ready
