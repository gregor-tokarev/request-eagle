# Packages the files staged by scripts/package-linux.sh for Fedora, openSUSE
# and other RPM distributions. The script defines version and staged_root.

%global app_id com.egortokarev.requesteagle
%global debug_package %{nil}

Name:           request-eagle
Version:        %{version}
Release:        1
Summary:        Fast, native API client
License:        MIT OR Apache-2.0
URL:            https://requesteagle.tokarev.work

# Vulkan, the GPU drivers and the Wayland client library are loaded at run
# time, so the automatic dependencies cannot see them.
Requires:       libvulkan.so.1()(64bit)
Requires:       libwayland-client.so.0()(64bit)
Recommends:     mesa-vulkan-drivers

%description
Request Eagle is a Postman alternative written in Rust and rendered on the
GPU. It keeps requests as TOML files on disk, with scripts, Vim mode and a
CLI for agents.

%install
cp -a %{staged_root}/. %{buildroot}/

%files
/usr/bin/request-eagle
/usr/share/applications/%{app_id}.desktop
/usr/share/icons/hicolor/512x512/apps/%{app_id}.png
/usr/share/metainfo/%{app_id}.metainfo.xml
%dir /usr/share/request-eagle
/usr/share/request-eagle/package
