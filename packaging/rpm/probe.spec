%undefine _debugsource_packages
%bcond_with prebuilt

Name:           probe
Version:        0.10.5
Release:        1%{?dist}
Summary:        Fast, native, local-first API client (CLI)

License:        MIT OR Apache-2.0
URL:            https://github.com/crizant/probe
Source0:        %{name}-%{version}.tar.gz

%if !%{with prebuilt}
BuildRequires:  cargo >= 1.95.0
BuildRequires:  rust >= 1.95.0
%endif
BuildRequires:  clang
BuildRequires:  alsa-lib-devel
BuildRequires:  fontconfig-devel
BuildRequires:  glib2-devel
%if 0%{?suse_version}
BuildRequires:  pkgconfig(vulkan)
%else
BuildRequires:  vulkan-loader-devel
%endif
BuildRequires:  wayland-devel
BuildRequires:  libX11-devel
BuildRequires:  libX11-xcb
BuildRequires:  libxcb-devel
BuildRequires:  libxkbcommon-devel
BuildRequires:  libxkbcommon-x11-devel
BuildRequires:  desktop-file-utils

%description
Probe is a fast, native, local-first API client built with Rust and
OpenCollection YAML.

This package provides the 'probe' command-line interface.

%package desktop
Summary:        Native desktop GUI client for Probe
Recommends:     %{name}%{?_isa} = %{version}-%{release}
%if 0%{?suse_version}
Requires:       libvulkan1
%else
Requires:       vulkan-loader%{?_isa}
%endif
Requires:       hicolor-icon-theme

%description desktop
Probe Desktop is a native GPUI application for interactive API development
and testing.

%prep
%autosetup -n %{name}-%{version} -p1

%build
%if !%{with prebuilt}
cargo build --release -p probe-cli --bin probe
cargo build --release -p probe-desktop --bin probe-desktop
%endif

%install
%if %{with prebuilt}
install -D -p -m 0755 %{prebuilt_cli} %{buildroot}%{_bindir}/probe
install -D -p -m 0755 %{prebuilt_desktop} %{buildroot}%{_bindir}/probe-desktop
%else
install -D -p -m 0755 target/release/probe %{buildroot}%{_bindir}/probe
install -D -p -m 0755 target/release/probe-desktop %{buildroot}%{_bindir}/probe-desktop
%endif

# Desktop entry
install -D -p -m 0644 packaging/linux/dev.probe.desktop.desktop \
    %{buildroot}%{_datadir}/applications/dev.probe.desktop.desktop

# Icons (16x16 through 512x512)
for size in 16 24 32 48 64 128 256 512; do
    icon_src="crates/desktop/assets/app-icon/linux/hicolor/${size}x${size}/apps/dev.probe.desktop.png"
    if [ -f "$icon_src" ]; then
        install -D -p -m 0644 "$icon_src" \
            "%{buildroot}%{_datadir}/icons/hicolor/${size}x${size}/apps/dev.probe.desktop.png"
    fi
done

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/dev.probe.desktop.desktop

%files
%license LICENSE
%doc README.md
%{_bindir}/probe

%files desktop
%license LICENSE
%{_bindir}/probe-desktop
%{_datadir}/applications/dev.probe.desktop.desktop
%{_datadir}/icons/hicolor/*/apps/dev.probe.desktop.png

%changelog
* Wed Oct 07 2026 Probe Contributors <support@probe.dev> - 0.10.5-1
- Initial RPM release for Probe CLI and Desktop
