# Packs a staged Blastit bundle (`make install PREFIX=<dir>`) as an RPM
# (`universal-distribution.md` §6.1).
#
# `scripts/build-rpm.sh` drives this spec; it defines `blastit_version`,
# `blastit_release`, `blastit_glibc` (the *measured* floor printed by
# `scripts/check-portable-elf.sh --max-glibc`) and `blastit_bundle` (the staged
# `/usr` tree). The spec's only job is to move that tree into the buildroot and
# declare the two runtime libraries; it never compiles anything.
#
# `AutoReqProv: no` is deliberate: RPM's automatic dependency generator would walk
# the shims and invent `libphp.so.*` / `libpython3.*` requirements for libraries
# Blastit `dlopen`s at run time and does not link. The package requires glibc and
# libgcc, exactly like `scripts/build-deb.sh`'s `Depends:`.
#
# `__os_install_post` is disabled so the payload stays byte-identical to the bundle
# that was measured: a strip or a build-id relink would make the floor a fact about
# a file nobody measured.

%global _build_id_links none
%global debug_package %{nil}
%global __os_install_post %{nil}

%{!?blastit_version:%global blastit_version 0.0.0}
%{!?blastit_release:%global blastit_release 1}
%{!?blastit_glibc:%global blastit_glibc 2.17}
# `0` when the measured `DT_NEEDED` set has no `libgcc_s.so.1`: the requirement is
# derived from the bundle, not from a list someone typed.
%{!?blastit_libgcc:%global blastit_libgcc 1}
%{!?blastit_bundle:%global blastit_bundle %{_builddir}/bundle}

Name: blastit
Version: %{blastit_version}
Release: %{blastit_release}
Summary: Zero-trust application harness and sandbox

License: Apache-2.0
URL: https://github.com/offworld-nexus/blastit
AutoReqProv: no
Requires: glibc >= %{blastit_glibc}
%if "%{blastit_libgcc}" == "1"
Requires: libgcc
%endif

%description
Blastit runs 12-factor web applications as isolated per-route sandboxes, validates
every inbound byte, and keeps credentials behind protocol-aware capability proxies.
This package ships the interpreter-free server, the language shims it loads at run
time, and the pure-Python helper package.

%prep

%build

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a %{blastit_bundle}/usr %{buildroot}/
# `cp -a` keeps the staging user; a package's files belong to root whoever built
# it (and `scripts/test_packages.sh` asserts `root:root` from the RPM header).
chown -R 0:0 %{buildroot}

%files
%{_bindir}/blastit
%{_bindir}/blastit-lsp
%{_datadir}/blastit
