# Upstream binary packages for the Probe CLI and desktop app.
#
# scripts/build-rpm.sh reads the version from Cargo.toml [workspace.package]
# and passes it as probe_version. Do not hard-code a version in this file.
# scripts/release.sh updates only Cargo.toml, which keeps this spec current.
#
# The release workflow supplies prebuilt Linux binaries. This spec does not
# compile Probe.

%global debug_package %{nil}
%global __os_install_post %{nil}
%undefine _debugsource_packages

Name:           probe
Version:        %{probe_version}
Release:        1
Summary:        Fast, native, local-first API client
License:        MIT OR Apache-2.0
URL:            https://github.com/crizant/probe

%description
Probe is a fast, native, local-first API client.

This package provides the probe command-line interface.

%package        desktop
Summary:        Native desktop client for Probe
Requires:       hicolor-icon-theme
Recommends:     %{name}%{?_isa} = %{version}-%{release}

%description    desktop
Probe Desktop is the native interface for interactive API work.
This package installs independently and includes its own license.
The command-line package is recommended alongside it.

%prep
cp -a "%{probe_readme}" README.md

%build
:

%install
install -D -p -m 0755 "%{probe_cli}" "%{buildroot}%{_bindir}/probe"
install -D -p -m 0755 "%{probe_desktop}" "%{buildroot}%{_bindir}/probe-desktop"
install -D -p -m 0644 "%{probe_license}" "%{buildroot}%{_datadir}/licenses/probe/LICENSE"
install -D -p -m 0644 "%{probe_license}" \
    "%{buildroot}%{_datadir}/licenses/probe-desktop/LICENSE"
install -D -p -m 0644 "%{probe_desktop_file}" \
    "%{buildroot}%{_datadir}/applications/dev.probe.desktop.desktop"
for size in 16 24 32 48 64 128 256 512; do
    src="%{probe_icon_root}/${size}x${size}/apps/dev.probe.desktop.png"
    test -f "${src}"
    install -D -p -m 0644 "${src}" \
        "%{buildroot}%{_datadir}/icons/hicolor/${size}x${size}/apps/dev.probe.desktop.png"
done

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/dev.probe.desktop.desktop

%files
%license %{_datadir}/licenses/probe/LICENSE
%doc README.md
%{_bindir}/probe

%files desktop
%license %{_datadir}/licenses/probe-desktop/LICENSE
%{_bindir}/probe-desktop
%{_datadir}/applications/dev.probe.desktop.desktop
%{_datadir}/icons/hicolor/*/apps/dev.probe.desktop.png

%changelog
* %{probe_changelog_date} Probe Contributors - %{probe_version}-1
- Upstream binary packages for the Probe CLI and desktop app.
