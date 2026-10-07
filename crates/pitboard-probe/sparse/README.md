# Sparse package for the identity block (VM K1)

A self-signed **sparse** package with an **external location** gives the unpackaged
`pitboard-probe-sparse.exe` a package identity without moving it into a package. Block K1
uses it to measure whether a sparse identity redirects writes under `%LOCALAPPDATA%\Pitboard`
and `HKCU`, whether the identity reaches the processes it starts, and whether disabling write
virtualization changes that. That decides APP's manifest and whether APP's build check
exists.

- `AppxManifest-virtualized.xml`: the documented sparse form (`Executable=`,
  `uap10:RuntimeBehavior="win32App"`, `uap10:TrustLevel="mediumIL"`), with write
  virtualization left as Windows sets it.
- `AppxManifest-unvirtualized.xml`: the same, with the `unvirtualizedResources` restricted
  capability and desktop6's `FileSystemWriteVirtualization` and
  `RegistryWriteVirtualization` set to `disabled` as elements of `Properties`.
- `k1.ps1`: trusts the certificate, packs, signs and registers either manifest, and removes
  both again. `register` sets `ProcessorArchitecture` to the VM's.
- `../manifests/sparse-identity.manifest`: the `<msix>` element embedded in
  `pitboard-probe-sparse.exe`, which ties the program to this package's name, publisher and
  application id, so running it from the external location gives it the identity.

**The VM only.** Register it only in a throwaway account there, and unregister it and remove
the certificate before reverting the snapshot.

## Steps

In the VM, with the four probe programs in `C:\probe` and this folder in `C:\probe\sparse`,
signed in at the console as **pbstd**, which carries the probe's throwaway marker. The
Windows SDK's `makeappx` and `signtool` must be installed. Run the **virtualized** manifest
first, then the **unvirtualized** one, each from a fresh revert to the **ready** snapshot.

1. In an **elevated** PowerShell (pbadmin's credentials at the prompt), once per manifest:

   ```powershell
   powershell -ExecutionPolicy Bypass -File C:\probe\sparse\k1.ps1 -Step trust
   ```

2. Back as pbstd, in a new PowerShell, register the package. Give `virtualized` or
   `unvirtualized`:

   ```powershell
   $S = Join-Path $env:TEMP 'pitboard-probe'
   New-Item -ItemType Directory -Force -Path $S | Out-Null
   powershell -ExecutionPolicy Bypass -File C:\probe\sparse\k1.ps1 -Step register -Manifest virtualized
   ```

3. From the identity process, confirm the identity, write, and start the children (the plain
   probe and a stand-in `claude.exe`, each writing a tag of its own):

   ```powershell
   C:\probe\pitboard-probe-sparse.exe sparse --action identity
   C:\probe\pitboard-probe-sparse.exe sparse --action write --tag identity-parent
   C:\probe\pitboard-probe-sparse.exe sparse --action children --scratch $S
   ```

4. From a process **without** identity, read everything back: the real folder, `HKCU`, and the
   package's redirected `LocalCache` folder:

   ```powershell
   C:\probe\pitboard-probe.exe sparse --action read
   ```

5. Clean up and unregister:

   ```powershell
   C:\probe\pitboard-probe.exe sparse --action clean
   powershell -ExecutionPolicy Bypass -File C:\probe\sparse\k1.ps1 -Step unregister
   ```

6. In an elevated PowerShell, remove the certificate, then revert to **ready**:

   ```powershell
   powershell -ExecutionPolicy Bypass -File C:\probe\sparse\k1.ps1 -Step untrust
   ```

`sparse --action write` writes a `<tag>.txt` under `%LOCALAPPDATA%\Pitboard\pitboard-probe-k1`,
renames it into place and reads it back, and writes and reads a value under
`HKCU\Software\pitboard-probe-k1`, and **leaves them** for `read`, so step 4 sees them, or
fails to if the identity redirected the write. `identity` reports
`has_application_user_model_id` and the call's raw `application_user_model_id_status`
(122 with an id, 15700 with no package, 15703 with a package but no application).

Send each `identity`, `write`, `children` and `read` object for both manifests, and say in
words whether the process without identity saw each tag.
