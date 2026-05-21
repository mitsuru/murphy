use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::mruby::{AstContext, MrubyState};
use crate::{Offense, Range, Severity};

pub const SANDBOX_POLICY_VERSION: &str = "sandbox-policy-v1";
pub const STDLIB_ALLOWLIST_VERSION: &str = "stdlib-allowlist-v1";

const DENIED_CAPABILITY_PROBES: &[(&str, &str)] = &[
    (
        "Kernel#system",
        "raise 'Kernel#system' if Kernel.private_instance_methods.include?(:system) || Kernel.instance_methods.include?(:system)",
    ),
    (
        "Kernel#`",
        "raise 'Kernel#`' if Kernel.private_instance_methods.include?(:`) || Kernel.instance_methods.include?(:`)",
    ),
    (
        "Kernel#load",
        "raise 'Kernel#load' if Kernel.private_instance_methods.include?(:load) || Kernel.instance_methods.include?(:load)",
    ),
    ("File", "raise 'File' if Object.const_defined?(:File)"),
    ("Dir", "raise 'Dir' if Object.const_defined?(:Dir)"),
    ("IO", "raise 'IO' if Object.const_defined?(:IO)"),
    ("Socket", "raise 'Socket' if Object.const_defined?(:Socket)"),
    (
        "Process",
        "raise 'Process' if Object.const_defined?(:Process)",
    ),
    ("ENV", "raise 'ENV' if Object.const_defined?(:ENV)"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxBootError {
    capability: String,
}

impl SandboxBootError {
    fn new(capability: impl Into<String>) -> Self {
        Self {
            capability: capability.into(),
        }
    }

    pub fn capability(&self) -> &str {
        &self.capability
    }
}

impl fmt::Display for SandboxBootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sandbox violation: {} is reachable", self.capability)
    }
}

impl std::error::Error for SandboxBootError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxViolation {
    capability: String,
}

impl SandboxViolation {
    fn new(capability: impl Into<String>) -> Self {
        Self {
            capability: capability.into(),
        }
    }

    pub fn capability(&self) -> &str {
        &self.capability
    }
}

impl fmt::Display for SandboxViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sandbox violation: {}", self.capability)
    }
}

impl std::error::Error for SandboxViolation {}

#[derive(Debug, Clone)]
pub struct SandboxPackage {
    package_id: String,
    root: PathBuf,
}

impl SandboxPackage {
    pub fn new(package_id: impl Into<String>, root: &Path) -> Result<Self, SandboxViolation> {
        let root = root
            .canonicalize()
            .map_err(|err| SandboxViolation::new(format!("package root: {err}")))?;
        Ok(Self {
            package_id: package_id.into(),
            root,
        })
    }

    pub fn package_id(&self) -> &str {
        &self.package_id
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn resolver(&self) -> RequireResolver<'_> {
        RequireResolver { package: self }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedRequireKind {
    MurphyStdlib,
    Package,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRequire {
    kind: ResolvedRequireKind,
    path: PathBuf,
}

impl ResolvedRequire {
    fn new(kind: ResolvedRequireKind, path: PathBuf) -> Self {
        Self { kind, path }
    }

    pub fn kind(&self) -> ResolvedRequireKind {
        self.kind
    }

    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }
}

pub struct RequireResolver<'a> {
    package: &'a SandboxPackage,
}

impl RequireResolver<'_> {
    pub fn resolve_require(&self, spec: &str) -> Result<ResolvedRequire, SandboxViolation> {
        if Path::new(spec).is_absolute() {
            return Err(SandboxViolation::new("absolute require path"));
        }
        if is_native_extension(spec) {
            return Err(SandboxViolation::new("native extension require"));
        }
        if is_allowlisted_stdlib(spec) {
            return Ok(ResolvedRequire::new(
                ResolvedRequireKind::MurphyStdlib,
                PathBuf::from("murphy_stdlib").join(format!("{spec}.rb")),
            ));
        }

        self.resolve_package_path(self.package.root.join(with_rb_extension(spec)))
    }

    pub fn resolve_require_relative(
        &self,
        spec: &str,
        from_file: &Path,
    ) -> Result<ResolvedRequire, SandboxViolation> {
        if Path::new(spec).is_absolute() {
            return Err(SandboxViolation::new("absolute require_relative path"));
        }
        if is_native_extension(spec) {
            return Err(SandboxViolation::new("native extension require"));
        }
        let base = from_file.parent().unwrap_or(self.package.root());
        self.resolve_package_path(base.join(with_rb_extension(spec)))
    }

    fn resolve_package_path(&self, path: PathBuf) -> Result<ResolvedRequire, SandboxViolation> {
        let canonical = path
            .canonicalize()
            .map_err(|err| SandboxViolation::new(format!("package require: {err}")))?;
        if !canonical.starts_with(self.package.root()) {
            return Err(SandboxViolation::new("package-root escape"));
        }
        if canonical.extension().and_then(|ext| ext.to_str()) != Some("rb") {
            return Err(SandboxViolation::new("non-ruby require"));
        }

        Ok(ResolvedRequire::new(
            ResolvedRequireKind::Package,
            canonical,
        ))
    }
}

fn is_allowlisted_stdlib(spec: &str) -> bool {
    matches!(spec, "json" | "set")
}

fn is_native_extension(spec: &str) -> bool {
    matches!(
        Path::new(spec).extension().and_then(|ext| ext.to_str()),
        Some("so" | "bundle" | "dll")
    )
}

fn with_rb_extension(spec: &str) -> PathBuf {
    let path = Path::new(spec);
    if path.extension().is_some() {
        path.to_path_buf()
    } else {
        PathBuf::from(format!("{spec}.rb"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageFingerprint(String);

impl PackageFingerprint {
    pub fn compute(package: &SandboxPackage) -> Result<Self, SandboxViolation> {
        let mut inputs = Vec::new();
        collect_fingerprint_inputs(package.root(), package.root(), &mut inputs)?;
        inputs.sort_by(|a, b| a.0.cmp(&b.0));

        let mut hasher = Sha256::new();
        for (rel, bytes) in inputs {
            hasher.update(rel.as_bytes());
            hasher.update([0]);
            hasher.update((bytes.len() as u64).to_be_bytes());
            hasher.update([0]);
            hasher.update(bytes);
            hasher.update([0xff]);
        }

        Ok(Self(format!("{:x}", hasher.finalize())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageCacheKey {
    package_id: String,
    package_fingerprint: PackageFingerprint,
    sandbox_policy_version: &'static str,
    stdlib_allowlist_version: &'static str,
}

impl PackageCacheKey {
    pub fn new(package: &SandboxPackage) -> Result<Self, SandboxViolation> {
        Ok(Self {
            package_id: package.package_id().to_owned(),
            package_fingerprint: PackageFingerprint::compute(package)?,
            sandbox_policy_version: SANDBOX_POLICY_VERSION,
            stdlib_allowlist_version: STDLIB_ALLOWLIST_VERSION,
        })
    }

    pub fn sandbox_policy_version(&self) -> &str {
        self.sandbox_policy_version
    }

    pub fn stdlib_allowlist_version(&self) -> &str {
        self.stdlib_allowlist_version
    }
}

fn collect_fingerprint_inputs(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), SandboxViolation> {
    for entry in
        fs::read_dir(dir).map_err(|err| SandboxViolation::new(format!("package read: {err}")))?
    {
        let entry = entry.map_err(|err| SandboxViolation::new(format!("package read: {err}")))?;
        let path = entry.path();
        let canonical = path
            .canonicalize()
            .map_err(|err| SandboxViolation::new(format!("package path: {err}")))?;
        if !canonical.starts_with(root) {
            return Err(SandboxViolation::new("package-root escape"));
        }
        let file_type = entry
            .file_type()
            .map_err(|err| SandboxViolation::new(format!("package file type: {err}")))?;
        if file_type.is_dir() {
            collect_fingerprint_inputs(root, &canonical, out)?;
            continue;
        }
        if !file_type.is_file() && !file_type.is_symlink() {
            continue;
        }
        if !is_fingerprint_input(&canonical) {
            continue;
        }
        let rel = canonical
            .strip_prefix(root)
            .map_err(|_| SandboxViolation::new("package-root escape"))?
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = fs::read(&canonical)
            .map_err(|err| SandboxViolation::new(format!("package read: {err}")))?;
        out.push((rel, bytes));
    }

    Ok(())
}

fn is_fingerprint_input(path: &Path) -> bool {
    if path.extension().and_then(|ext| ext.to_str()) == Some("rb") {
        return true;
    }

    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("murphy.toml" | "package.toml" | "murphy-package.toml")
    )
}

pub fn validate_denied_capabilities_absent(
    reachable: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<(), SandboxBootError> {
    if let Some(capability) = reachable.into_iter().next() {
        return Err(SandboxBootError::new(capability.as_ref()));
    }

    Ok(())
}

pub fn boot_self_check(state: &mut MrubyState) -> Result<(), SandboxBootError> {
    for (capability, probe) in DENIED_CAPABILITY_PROBES {
        if state.eval_checked(probe) {
            return Err(SandboxBootError::new(*capability));
        }
    }

    Ok(())
}

pub fn run_mruby_cop_sandboxed(
    ctx: &Arc<AstContext>,
    cop_source: &str,
    cop_name: &str,
    file: &str,
) -> Vec<Offense> {
    let sandboxed_source = sandboxed_cop_source(cop_source);
    let mut offenses = crate::mruby::run_mruby_cop_isolated(ctx, &sandboxed_source, cop_name, file);
    rewrite_isolated_exception_as_sandbox_violation(&mut offenses);
    offenses
}

pub fn run_mruby_cop_sandboxed_with_package(
    ctx: &Arc<AstContext>,
    package: &SandboxPackage,
    cop_path: &Path,
    cop_source: &str,
    cop_name: &str,
    file: &str,
) -> Vec<Offense> {
    match expand_package_requires(package, cop_path, cop_source) {
        Ok(expanded) => run_mruby_cop_sandboxed(ctx, &expanded, cop_name, file),
        Err(err) => vec![sandbox_error_offense(file, cop_name, err.capability())],
    }
}

fn expand_package_requires(
    package: &SandboxPackage,
    from_file: &Path,
    source: &str,
) -> Result<String, SandboxViolation> {
    let mut seen = HashSet::new();
    expand_package_requires_inner(package, from_file, source, &mut seen)
}

fn expand_package_requires_inner(
    package: &SandboxPackage,
    from_file: &Path,
    source: &str,
    seen: &mut HashSet<PathBuf>,
) -> Result<String, SandboxViolation> {
    let resolver = package.resolver();
    let mut out = String::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if let Some(spec) = quoted_require_arg(trimmed, "require_relative") {
            let resolved = resolver.resolve_require_relative(spec, from_file)?;
            append_resolved_require(package, &resolved, &mut out, seen)?;
            continue;
        }
        if let Some(spec) = quoted_require_arg(trimmed, "require") {
            let resolved = resolver.resolve_require(spec)?;
            append_resolved_require(package, &resolved, &mut out, seen)?;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }

    Ok(out)
}

fn append_resolved_require(
    package: &SandboxPackage,
    resolved: &ResolvedRequire,
    out: &mut String,
    seen: &mut HashSet<PathBuf>,
) -> Result<(), SandboxViolation> {
    if resolved.kind() == ResolvedRequireKind::MurphyStdlib {
        return Ok(());
    }

    let path = resolved.path();
    if !seen.insert(path.clone()) {
        return Ok(());
    }
    let source = fs::read_to_string(&path)
        .map_err(|err| SandboxViolation::new(format!("package require read: {err}")))?;
    out.push_str(&expand_package_requires_inner(
        package, &path, &source, seen,
    )?);
    Ok(())
}

fn quoted_require_arg<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(keyword)?.trim_start();
    let quote = rest.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let rest = &rest[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

fn sandboxed_cop_source(cop_source: &str) -> String {
    format!(
        r#"
{SANDBOX_RUNTIME_PRELUDE}
begin
{cop_source}
rescue Exception => e
  raise "Sandbox violation: #{{e.message}}"
end
"#
    )
}

const SANDBOX_RUNTIME_PRELUDE: &str = r#"
class Object
  [:File, :Dir, :IO, :Socket, :Process, :ENV, :Open3].each do |name|
    remove_const(name) if const_defined?(name)
  end
end

module Kernel
  [:system, :`, :exec, :spawn, :load].each do |name|
    begin
      undef_method(name) if Kernel.private_instance_methods.include?(name) || Kernel.instance_methods.include?(name)
    rescue Exception
    end
  end

  def require(name)
    case name.to_s
    when "json", "set"
      true
    else
      raise "Sandbox violation: require #{name}"
    end
  end

  def require_relative(name)
    raise "Sandbox violation: require_relative #{name}"
  end
end

[:File, :Dir, :IO, :Socket, :Process, :ENV, :Open3].each do |name|
  raise "Sandbox violation: #{name}" if Object.const_defined?(name)
end
"#;

fn rewrite_isolated_exception_as_sandbox_violation(offenses: &mut [Offense]) {
    if let [offense] = offenses
        && offense.severity == Severity::Error
        && offense
            .message
            .contains("raised an exception (isolated; design")
    {
        offense.message = "Sandbox violation: denied capability".to_owned();
        offense.range = Range {
            start_offset: 0,
            end_offset: 0,
        };
    }
}

fn sandbox_error_offense(file: &str, cop_name: &str, capability: &str) -> Offense {
    Offense::new(
        file,
        cop_name,
        Range {
            start_offset: 0,
            end_offset: 0,
        },
        Severity::Error,
        &format!("Sandbox violation: {capability}"),
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::mruby::MrubyState;
    use tempfile::tempdir;

    #[test]
    fn boot_self_check_rejects_default_runtime_when_denied_capability_is_reachable() {
        let mut state = MrubyState::open();

        let err =
            super::boot_self_check(&mut state).expect_err("default mruby exposes Kernel#system");

        assert_eq!(err.capability(), "Kernel#system");
    }

    #[test]
    fn denied_capability_policy_accepts_empty_reachable_set() {
        super::validate_denied_capabilities_absent(Vec::<&str>::new())
            .expect("no denied APIs are reachable");
    }

    #[test]
    fn require_resolver_accepts_package_relative_rb_under_root() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("lib")).expect("create lib");
        fs::write(dir.path().join("lib/helper.rb"), "HELPER = true\n").expect("write helper");
        let package = super::SandboxPackage::new("pkg", dir.path()).expect("package");

        let resolved = package
            .resolver()
            .resolve_require_relative("lib/helper", dir.path().join("cop.rb").as_path())
            .expect("relative helper is allowed");

        assert_eq!(resolved.kind(), super::ResolvedRequireKind::Package);
        assert_eq!(
            resolved.path(),
            dir.path().join("lib/helper.rb").canonicalize().unwrap()
        );
    }

    #[test]
    fn require_resolver_rejects_absolute_root_escape_and_native_extension() {
        let dir = tempdir().expect("tempdir");
        let outside = tempdir().expect("outside tempdir");
        fs::write(outside.path().join("evil.rb"), "EVIL = true\n").expect("write evil");
        fs::write(dir.path().join("native.so"), "not really native\n").expect("write native");
        let package = super::SandboxPackage::new("pkg", dir.path()).expect("package");
        let resolver = package.resolver();

        assert!(
            resolver
                .resolve_require(outside.path().join("evil.rb").to_str().unwrap())
                .is_err()
        );
        assert!(
            resolver
                .resolve_require_relative("../evil", dir.path().join("cop.rb").as_path())
                .is_err()
        );
        assert!(
            resolver
                .resolve_require_relative("native.so", dir.path().join("cop.rb").as_path())
                .is_err()
        );
    }

    #[test]
    fn require_resolver_prevents_package_stdlib_shadowing() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("json.rb"), "SHADOW = true\n").expect("write json shadow");
        let package = super::SandboxPackage::new("pkg", dir.path()).expect("package");

        let resolved = package
            .resolver()
            .resolve_require("json")
            .expect("allowlisted stdlib resolves before package-local shadow");

        assert_eq!(resolved.kind(), super::ResolvedRequireKind::MurphyStdlib);
    }

    #[test]
    fn package_fingerprint_changes_for_content_and_path_changes() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("a.rb"), "A = 1\n").expect("write a");
        let package = super::SandboxPackage::new("pkg", dir.path()).expect("package");
        let original = super::PackageFingerprint::compute(&package).expect("fingerprint");

        fs::write(dir.path().join("a.rb"), "A = 2\n").expect("modify a");
        let content_changed = super::PackageFingerprint::compute(&package).expect("fingerprint");
        assert_ne!(original, content_changed);

        fs::write(dir.path().join("a.rb"), "A = 1\n").expect("restore a");
        fs::rename(dir.path().join("a.rb"), dir.path().join("b.rb")).expect("rename");
        let path_changed = super::PackageFingerprint::compute(&package).expect("fingerprint");
        assert_ne!(original, path_changed);
    }

    #[test]
    fn package_cache_key_includes_identity_fingerprint_and_policy_versions() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("cop.rb"), "class Cop; end\n").expect("write cop");
        let package_a = super::SandboxPackage::new("pkg-a", dir.path()).expect("package a");
        let package_b = super::SandboxPackage::new("pkg-b", dir.path()).expect("package b");

        let key_a = super::PackageCacheKey::new(&package_a).expect("key a");
        let key_b = super::PackageCacheKey::new(&package_b).expect("key b");

        assert_ne!(key_a, key_b);
        assert_eq!(
            key_a.sandbox_policy_version(),
            super::SANDBOX_POLICY_VERSION
        );
        assert_eq!(
            key_a.stdlib_allowlist_version(),
            super::STDLIB_ALLOWLIST_VERSION
        );
    }

    #[cfg(unix)]
    #[test]
    fn package_fingerprint_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().expect("tempdir");
        let outside = tempdir().expect("outside");
        fs::write(outside.path().join("evil.rb"), "EVIL = true\n").expect("write evil");
        symlink(outside.path().join("evil.rb"), dir.path().join("evil.rb")).expect("symlink");
        let package = super::SandboxPackage::new("pkg", dir.path()).expect("package");

        let err = super::PackageFingerprint::compute(&package).expect_err("escape rejected");

        assert!(err.to_string().contains("package-root escape"));
    }

    #[test]
    fn sandbox_denial_file_read_becomes_one_error_offense() {
        let ctx = crate::mruby::AstContext::new(b"puts 'hi'\n".to_vec());
        let offenses = super::run_mruby_cop_sandboxed(
            &ctx,
            "File.read('/etc/passwd')\nclass Bad < Murphy::Cop; end\n",
            "ThirdParty/Bad",
            "sample.rb",
        );

        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].severity, crate::Severity::Error);
        assert_eq!(offenses[0].cop_name, "ThirdParty/Bad");
        assert!(offenses[0].message.starts_with("Sandbox violation:"));
    }

    #[test]
    fn sandbox_denial_dynamic_file_constant_bypass_becomes_one_error_offense() {
        let ctx = crate::mruby::AstContext::new(b"puts 'hi'\n".to_vec());
        let offenses = super::run_mruby_cop_sandboxed(
            &ctx,
            "Object.const_get(:File).read('/etc/passwd')\nclass Bad < Murphy::Cop; end\n",
            "ThirdParty/Bad",
            "sample.rb",
        );

        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].severity, crate::Severity::Error);
        assert!(offenses[0].message.starts_with("Sandbox violation:"));
    }

    #[test]
    fn sandbox_denial_dynamic_system_send_becomes_one_error_offense() {
        let ctx = crate::mruby::AstContext::new(b"puts 'hi'\n".to_vec());
        let offenses = super::run_mruby_cop_sandboxed(
            &ctx,
            "send(:system, 'true')\nclass Bad < Murphy::Cop; end\n",
            "ThirdParty/System",
            "sample.rb",
        );

        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].severity, crate::Severity::Error);
        assert!(offenses[0].message.starts_with("Sandbox violation:"));
    }

    #[test]
    fn sandbox_denial_require_socket_becomes_one_error_offense() {
        let ctx = crate::mruby::AstContext::new(b"puts 'hi'\n".to_vec());
        let offenses = super::run_mruby_cop_sandboxed(
            &ctx,
            "require 'socket'\nclass Bad < Murphy::Cop; end\n",
            "ThirdParty/Socket",
            "sample.rb",
        );

        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.starts_with("Sandbox violation:"));
    }

    #[test]
    fn sandbox_denial_does_not_poison_sibling_good_cop() {
        let ctx = crate::mruby::AstContext::new(b"puts 'hi'\n".to_vec());
        let bad = super::run_mruby_cop_sandboxed(
            &ctx,
            "ENV.to_h\nclass Bad < Murphy::Cop; end\n",
            "ThirdParty/Bad",
            "sample.rb",
        );
        let good = super::run_mruby_cop_sandboxed(
            &ctx,
            "class Good < Murphy::Cop\n  def on_call_node(node)\n    add_offense(node.message_loc, message: 'good')\n  end\nend\n",
            "ThirdParty/Good",
            "sample.rb",
        );

        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].severity, crate::Severity::Error);
        assert_eq!(good.len(), 1);
        assert_eq!(good[0].message, "good");
    }

    #[test]
    fn sandbox_package_runner_loads_package_relative_helper_through_resolver() {
        let dir = tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join("lib")).expect("create lib");
        fs::write(
            dir.path().join("lib/helper.rb"),
            "HELPER_MESSAGE = 'from helper'\n",
        )
        .expect("write helper");
        let cop_path = dir.path().join("cop.rb");
        let package = super::SandboxPackage::new("pkg", dir.path()).expect("package");
        let ctx = crate::mruby::AstContext::new(b"puts 'hi'\n".to_vec());

        let offenses = super::run_mruby_cop_sandboxed_with_package(
            &ctx,
            &package,
            &cop_path,
            "require_relative 'lib/helper'\nclass Good < Murphy::Cop\n  def on_call_node(node)\n    add_offense(node.message_loc, message: HELPER_MESSAGE)\n  end\nend\n",
            "ThirdParty/Good",
            "sample.rb",
        );

        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "from helper");
    }
}
