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
the certificate before reverting the snapshot. The steps, with the commands, are block K1 of
the VM session's runbook.
