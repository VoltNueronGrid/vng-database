# VoltNueronGrid Perl Driver — Feasibility Report

**Date**: 2026-04-22
**Author**: VoltNueronGrid Platform Team
**Status**: Deferred — pending C FFI layer (S10-003) GA

---

## 1. Approach Options

### Option A: XS Extension (C → Rust cdylib via XS glue)

Write a Perl XS module that links directly against `libvoltnuerongrid_driver.so`
(produced by S10-003 `vng-driver-c`).

**Pros:**
- Maximum performance; calls arrive directly in Rust with no interpreter overhead.
- Full control over memory layout between Perl SV and Rust structures.

**Cons:**
- XS is notoriously verbose and error-prone; requires expert-level C and Perl internals knowledge.
- Binary distribution requires a C compiler on end-user systems (`ExtUtils::MakeMaker` / `Dist::Zilla`).
- Rebuild required on each Perl minor version (`.so` ABI is Perl-version-specific).
- Estimated effort: 5–6 weeks for a minimal read-only driver.

**Verdict:** Viable for a production-grade driver but over-engineered for an initial binding.

---

### Option B: FFI::Platypus (load .so at runtime — no XS required)

Use the CPAN module `FFI::Platypus` to dynamically load the shared library at
runtime and bind C function signatures declared in Perl.

**Pros:**
- No C compiler required on end-user systems; install the shared `.so` separately.
- Pure Perl code; readable, maintainable, no XS boilerplate.
- Works with the same `voltnuerongrid.h` C signatures from S10-003.
- `FFI::Platypus::Record` handles C structs (`VngRequest`) directly.
- Binary re-use: the same `.so` serves Python ctypes, Ruby FFI, Go cgo, etc.

**Cons:**
- Slight runtime overhead vs. compiled XS (negligible for I/O-bound DB calls).
- `FFI::Platypus` is a CPAN dependency (though well-maintained; Perl 5.10+).
- Requires `libvoltnuerongrid_driver.so` on the `$LD_LIBRARY_PATH` at runtime.

**Verdict:** Recommended path. Best effort-to-value trade-off.

---

### Option C: REST-only Pure Perl

Implement the full driver logic in pure Perl using `LWP::UserAgent` (or
`HTTP::Tiny` from core) to call the REST endpoints directly, with no FFI.

**Pros:**
- Zero native dependencies; ships as a pure `.pm` file.
- Works on any Perl 5.10+ installation without a C toolchain.
- Simplest distribution story (CPAN module, no shared library).

**Cons:**
- Duplicates all driver logic (validation, header construction, retry, SQL routing).
- Must be kept in sync with the Rust/TypeScript/Java reference implementations manually.
- Does not benefit from future native wire protocol (vng://) support.

**Verdict:** Acceptable for a quick prototype or constrained environments. Not recommended as the primary path.

---

## 2. Recommended Path: FFI::Platypus Binding to the C FFI Layer (S10-003)

The `FFI::Platypus`-based approach (Option B) provides:

- A pure Perl module that is easy to install (`cpanm VoltNueronGrid::Driver`).
- Automatic alignment with the canonical C ABI; any improvements to the Rust
  core are immediately available to Perl consumers after a library update.
- A clear upgrade path to the native wire protocol once the C FFI layer exposes it.

### Architecture

```
Perl caller
  └── VoltNueronGrid::Driver  (Perl module, FFI::Platypus)
        └── libvoltnuerongrid_driver.so  (S10-003, Rust cdylib)
              └── voltnuerongrid-driver-rust / core crates
```

---

## 3. Sample Code

```perl
package VoltNueronGrid::Driver;

use strict;
use warnings;
use FFI::Platypus 2.00;
use FFI::Platypus::Record;

# Locate the shared library (distribute alongside the Perl module,
# or let users set VNG_DRIVER_LIB_PATH).
my $lib = $ENV{VNG_DRIVER_LIB_PATH} // 'libvoltnuerongrid_driver.so';

my $ffi = FFI::Platypus->new(api => 2);
$ffi->lib($lib);

# Map the opaque handle to a Perl pointer type
$ffi->type('opaque' => 'VngDriverHandle');

# Map the VngRequest C struct
FFI::Platypus::Record->import;
record_layout_1(
    $ffi,
    int    => 'method',
    string => 'url',
    string => 'headers_json',
    string => 'body_json',
);
$ffi->type('record(VngRequest)' => 'VngRequest');

# Bind C functions
$ffi->attach(vng_driver_create => ['string', 'string', 'string'] => 'VngDriverHandle');
$ffi->attach(vng_driver_free   => ['VngDriverHandle']             => 'void');
$ffi->attach(vng_driver_build_health_request =>
    ['VngDriverHandle', 'VngRequest'] => 'int');
$ffi->attach(vng_request_free => ['VngRequest'] => 'void');

sub new {
    my ($class, %args) = @_;
    my $handle = vng_driver_create(
        $args{base_url}   // die("base_url required"),
        $args{session_id} // die("session_id required"),
        $args{mode}       // 'admin',
    ) or die "vng_driver_create failed";
    return bless { _handle => $handle }, $class;
}

sub build_health_request {
    my ($self) = @_;
    my $req = VngRequest->new;
    vng_driver_build_health_request($self->{_handle}, $req) == 0
        or die "build_health_request failed";
    my %result = (
        method  => $req->method == 0 ? 'GET' : 'POST',
        url     => $req->url,
        headers => $req->headers_json,
    );
    vng_request_free($req);
    return \%result;
}

sub DESTROY {
    my ($self) = @_;
    vng_driver_free($self->{_handle}) if $self->{_handle};
}

1;
```

Usage:

```perl
use VoltNueronGrid::Driver;

my $drv = VoltNueronGrid::Driver->new(
    base_url   => 'http://localhost:8080',
    session_id => 'perl-session',
    mode       => 'admin',
);

my $req = $drv->build_health_request();
print "URL: $req->{url}\n";
```

---

## 4. Effort Estimate

| Phase | Description | Estimate |
|-------|-------------|----------|
| P1 | FFI bindings for handle lifecycle + health + execute | 3 days |
| P2 | All 6 request builders + validation error mapping | 3 days |
| P3 | `HTTP::Tiny`-based execution layer with retry | 4 days |
| P4 | CPAN-compatible distribution (`Makefile.PL`, tests, pod) | 4 days |
| P5 | Integration tests against a running server | 3 days |
| **Total** | Read-only driver, basic test coverage | **~2 weeks** |

A full-featured driver with native wire protocol support would add another 2–3 weeks
once the C FFI layer exposes the native transport.

---

## 5. Decision: Deferred

**The Perl binding is deferred until the C FFI layer (S10-003) reaches General Availability (GA).**

Rationale:

1. The C FFI surface is the stable foundation for all FFI-based bindings (Perl, Ruby, Python).
   Binding to an in-flux API would require rework.

2. The REST-only pure Perl option can serve immediate needs with very low effort
   if a Perl consumer is required before S10-003 GA.

3. Perl is not in the current top-3 driver priority list; Java (S10-001),
   Node.js (S10-002), and Deno (S10-004) cover the immediate demand.

**Trigger to revisit:** `vng-driver-c` tagged as GA in the release tracker.

---

*End of feasibility report.*
