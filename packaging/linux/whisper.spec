# Built by `cargo xtask rpm` from an already staged /usr tree (%{whisper_root}).
Name:           whisper-browser
Version:        %{whisper_version}
Release:        1
Summary:        Quiet browser built on Firefox
License:        MPL-2.0
URL:            https://github.com/footprintsntrails/whisper-browser
Recommends:     firefox
AutoReqProv:    no

%global debug_package %{nil}
%global __os_install_post %{nil}
%global __strip /bin/true
%define _build_id_links none

%description
Whisper starts your installed Firefox with a separate, quiet profile: telemetry and
studies off, uBlock Origin from addons.mozilla.org, DNS over HTTPS, Strict tracking
protection and a dark theme with tabs on the side. Your normal Firefox profile is not
touched. Whisper can't make you invisible. It just won't snitch.

%install
mkdir -p %{buildroot}
cp -a %{whisper_root}/. %{buildroot}/

%files
/usr/bin/whisper
/usr/share/whisper
/usr/share/applications/whisper.desktop
/usr/share/icons/hicolor/256x256/apps/whisper.png
/usr/share/icons/hicolor/512x512/apps/whisper.png
/usr/share/icons/hicolor/scalable/apps/whisper.svg
/usr/share/doc/whisper-browser
