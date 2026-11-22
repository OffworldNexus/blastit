# Packs a staged Blastit bundle (`make install PREFIX=<dir>`) as seven RPM
# packages (`universal-distribution.md` §6.1, `MB.65`).
#
# `scripts/build-rpm.sh` drives this spec; it defines `blastit_version`,
# `blastit_release`, `blastit_bundle` (the staged `/usr` tree) and — one pair per
# package — `blastit_glibc_<tag>`/`blastit_libgcc_<tag>`: the floor
# `scripts/check-portable-elf.sh --max-glibc` measured **for that package's own
# ELFs** and whether its measured `DT_NEEDED` set needs `libgcc_s.so.1`. The tags
# are `core`, `py313`, `py314`, `php83`, `php84`, `php85`, `php86`. The spec's
# only job is to move that tree into the buildroot and declare those measured
# relationships; it never compiles anything.
#
# # One floor per package, never the bundle's maximum
#
# `scripts/build-deb.sh` measures `blastit`+`blastit-lsp` for the core and each
# shim for its own package, and this spec is the RPM mirror of that: the core
# `Requires: glibc >= %{blastit_glibc_core}` (2.34 on today's artifacts) and
# `blastit-php-8.4` `Requires: glibc >= %{blastit_glibc_php84}` (2.17), so a
# shim does not demand a newer glibc than its own bytes need. `libgcc` is
# declared only where the measured `DT_NEEDED` set carries `libgcc_s.so.1`
# (the binaries and the CPython shims; not the PHP shims).
#
# `AutoReqProv: no` is deliberate: RPM's automatic dependency generator would
# walk the shims and invent `libphp.so.*` / `libpython3.*` requirements for
# libraries Blastit `dlopen`s at run time and does not link. It is a *package*
# header, so every `%package -n` below restates it as `AutoReq: no`/`AutoProv:
# no` — without that, the subpackages (and only they) get generated
# `libc.so.6(GLIBC_2.x)` requirements that nobody measured.
#
# # The interpreter relationship is weak, and has to be
#
# Every shim package — `blastit-python-*` and `blastit-php-*` — carries the
# matching distro runtime as a **`Recommends:`**, exactly as
# `scripts/build-deb.sh`'s control files do (`Recommends: python3.13` /
# `Recommends: libphp8.4-embed` there, `Recommends: python3.13` /
# `Recommends: php-embedded` here, because RPM distributions name no interpreter
# package per minor). RPM distributions cannot back a hard requirement: Rocky 9
# has **no** `python3.13` (it ships 3.9) and its `php-embedded` is 8.0, so
# `Requires: python3.13` or `Requires: php-embedded` would make the shim package
# uninstallable on a distribution the release still supports. `Recommends:`
# states the intent, shows in `dnf info`, is pulled by a plain `dnf install` when
# available and skipped when it is not, and leaves the operator in control. A
# host that follows the docs installs the matching interpreter first.
#
# `__os_install_post` is disabled so the payload stays byte-identical to the
# bundle that was measured: a strip or a build-id relink would make the floor a
# fact about a file nobody measured.

%global _build_id_links none
%global debug_package %{nil}
%global __os_install_post %{nil}

%{!?blastit_version:%global blastit_version 0.0.0}
%{!?blastit_release:%global blastit_release 1}
%{!?blastit_bundle:%global blastit_bundle %{_builddir}/bundle}

# Per-package measured floors, one pair each (see the header). The defaults are
# the oldest glibc a gnu artifact may claim; `build-rpm.sh` always passes the
# measured value, so a default only shows up in a hand-run rpmbuild.
%{!?blastit_glibc_core:%global blastit_glibc_core 2.17}
%{!?blastit_libgcc_core:%global blastit_libgcc_core 0}
%{!?blastit_glibc_py313:%global blastit_glibc_py313 2.17}
%{!?blastit_libgcc_py313:%global blastit_libgcc_py313 0}
%{!?blastit_glibc_py314:%global blastit_glibc_py314 2.17}
%{!?blastit_libgcc_py314:%global blastit_libgcc_py314 0}
%{!?blastit_glibc_php83:%global blastit_glibc_php83 2.17}
%{!?blastit_libgcc_php83:%global blastit_libgcc_php83 0}
%{!?blastit_glibc_php84:%global blastit_glibc_php84 2.17}
%{!?blastit_libgcc_php84:%global blastit_libgcc_php84 0}
%{!?blastit_glibc_php85:%global blastit_glibc_php85 2.17}
%{!?blastit_libgcc_php85:%global blastit_libgcc_php85 0}
%{!?blastit_glibc_php86:%global blastit_glibc_php86 2.17}
%{!?blastit_libgcc_php86:%global blastit_libgcc_php86 0}

Name: blastit
Version: %{blastit_version}
Release: %{blastit_release}
Summary: Zero-trust application harness and sandbox

License: Apache-2.0
URL: https://github.com/offworld-nexus/blastit
AutoReqProv: no
Requires: glibc >= %{blastit_glibc_core}
%if "%{blastit_libgcc_core}" == "1"
Requires: libgcc
%endif

%description
Blastit runs 12-factor web applications as isolated per-route sandboxes, validates
every inbound byte, and keeps credentials behind protocol-aware capability proxies.
This package ships the interpreter-free server, the native language server, the
licence and the pure-Python helper package; the language shims are the
`blastit-python-*` and `blastit-php-*` subpackages beside it.

%package -n blastit-python-3.13
Summary: CPython 3.13 view shim for blastit
Requires: glibc >= %{blastit_glibc_py313}
%if "%{blastit_libgcc_py313}" == "1"
Requires: libgcc
%endif
Requires: blastit = %{version}-%{release}
AutoReq: no
AutoProv: no
Recommends: python3.13

%description -n blastit-python-3.13
The libblastit_py313 shim lets blastit run Python WSGI views on CPython 3.13. It is
loaded into the host interpreter by the blastit binary; install it together with
blastit, never instead of it.

%package -n blastit-python-3.14
Summary: CPython 3.14 view shim for blastit
Requires: glibc >= %{blastit_glibc_py314}
%if "%{blastit_libgcc_py314}" == "1"
Requires: libgcc
%endif
Requires: blastit = %{version}-%{release}
AutoReq: no
AutoProv: no
Recommends: python3.14

%description -n blastit-python-3.14
The libblastit_py314 shim lets blastit run Python WSGI views on CPython 3.14. It is
loaded into the host interpreter by the blastit binary; install it together with
blastit, never instead of it.

%package -n blastit-php-8.3
Summary: PHP 8.3 embedded SAPI shim for blastit
Requires: glibc >= %{blastit_glibc_php83}
%if "%{blastit_libgcc_php83}" == "1"
Requires: libgcc
%endif
Requires: blastit = %{version}-%{release}
AutoReq: no
AutoProv: no
Recommends: php-embedded

%description -n blastit-php-8.3
The libblastit_php83 shim lets blastit run PHP view scripts through the host's
embedded NTS libphp 8.3. It is loaded by the blastit binary; install it together
with blastit and a PHP 8.3 embedded build, never instead of it.

%package -n blastit-php-8.4
Summary: PHP 8.4 embedded SAPI shim for blastit
Requires: glibc >= %{blastit_glibc_php84}
%if "%{blastit_libgcc_php84}" == "1"
Requires: libgcc
%endif
Requires: blastit = %{version}-%{release}
AutoReq: no
AutoProv: no
Recommends: php-embedded

%description -n blastit-php-8.4
The libblastit_php84 shim lets blastit run PHP view scripts through the host's
embedded NTS libphp 8.4. It is loaded by the blastit binary; install it together
with blastit and a PHP 8.4 embedded build, never instead of it.

%package -n blastit-php-8.5
Summary: PHP 8.5 embedded SAPI shim for blastit
Requires: glibc >= %{blastit_glibc_php85}
%if "%{blastit_libgcc_php85}" == "1"
Requires: libgcc
%endif
Requires: blastit = %{version}-%{release}
AutoReq: no
AutoProv: no
Recommends: php-embedded

%description -n blastit-php-8.5
The libblastit_php85 shim lets blastit run PHP view scripts through the host's
embedded NTS libphp 8.5. It is loaded by the blastit binary; install it together
with blastit and a PHP 8.5 embedded build, never instead of it.

%package -n blastit-php-8.6
Summary: PHP 8.6 embedded SAPI shim for blastit
Requires: glibc >= %{blastit_glibc_php86}
%if "%{blastit_libgcc_php86}" == "1"
Requires: libgcc
%endif
Requires: blastit = %{version}-%{release}
AutoReq: no
AutoProv: no
Recommends: php-embedded

%description -n blastit-php-8.6
The libblastit_php86 shim lets blastit run PHP view scripts through the host's
embedded NTS libphp 8.6. It is loaded by the blastit binary; install it together
with blastit and a PHP 8.6 embedded build, never instead of it.

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
%{_datadir}/blastit/LICENSE
%{_datadir}/blastit/python
%{_datadir}/blastit/lib/blastit
%{_datadir}/blastit/lib/blastit-lsp

%files -n blastit-python-3.13
%{_datadir}/blastit/lib/libblastit_py313-*.so

%files -n blastit-python-3.14
%{_datadir}/blastit/lib/libblastit_py314-*.so

%files -n blastit-php-8.3
%{_datadir}/blastit/lib/libblastit_php83-*.so

%files -n blastit-php-8.4
%{_datadir}/blastit/lib/libblastit_php84-*.so

%files -n blastit-php-8.5
%{_datadir}/blastit/lib/libblastit_php85-*.so

%files -n blastit-php-8.6
%{_datadir}/blastit/lib/libblastit_php86-*.so
