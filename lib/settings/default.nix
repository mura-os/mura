# lib/settings — the settings-schema compiler (specs/settings-schema.md §1; specs/settings-daemon.md).
#
# GSettings compiles `.gschema.xml` into `gschemas.compiled` so that no application carries a
# default of its own (research/58 §1). Mura's schema source is the evaluated NixOS option tree:
# an option annotated with `mkSetting` becomes a key record in `/etc/mura/settings-schema.json`,
# its post-priority evaluated value the key's default (constraint 9: the artifact is the only
# default channel). Templates (relocatable schemas, §1.1) are declared in `mura.settings.templates`.
{ lib }:
let
  inherit (lib) mkOption isOption isAttrs concatLists mapAttrsToList optionalAttrs getAttrFromPath;

  # `mkOption` has a closed argument set (nixpkgs lib/options.nix); an extra attribute merged onto
  # the resulting option survives `mergeOptionDecls` (verified in the D7 prototype), so the
  # annotation rides on the option itself and no second registry can drift from it.
  mkSetting = args:
    assert lib.assertMsg (args ? settings) "mkSetting: `settings` is required";
    assert lib.assertMsg (args.settings ? schema && args.settings ? key) "mkSetting: settings.schema and settings.key are required";
    (mkOption (removeAttrs args [ "settings" ])) // { muraSettings = args.settings; };

  # Walk an option tree for annotated options.
  collect = prefix: opts: concatLists (mapAttrsToList
    (n: o:
      if isOption o then (if o ? muraSettings then [{ path = prefix ++ [ n ]; opt = o; }] else [ ])
      else if isAttrs o && !(o ? _type) then collect (prefix ++ [ n ]) o
      else [ ])
    opts);

  # The artifact's type vocabulary (§1): bool | int | double | string | enum.
  typeRecord = t:
    let n = t.name; in
    if n == "bool" then { type = "bool"; }
    else if n == "int" || n == "signedInt" || n == "unsignedInt" || n == "intBetween" then { type = "int"; }
    else if n == "float" then { type = "double"; }
    else if n == "str" || n == "string" || n == "nonEmptyStr" then { type = "string"; }
    else if n == "enum" then { type = "enum"; values = t.functor.payload.values; }
    else throw "lib/settings: unsupported option type `${n}` for a settings key (bool, int, float, str, enum)";

  # A key record from an annotated option and the evaluated configuration.
  keyRecord = { locks, config }: { path, opt }:
    let
      s = opt.muraSettings;
      mutability = if lib.isFunction (s.mutability or null) then s.mutability config.mura else (s.mutability or "immutable");
      id = "${s.schema}.${s.key}";
    in
    assert lib.assertMsg (lib.elem (s.class or "preference") [ "preference" "state" ]) "settings ${id}: class must be preference | state";
    assert lib.assertMsg (lib.elem (s.stratum or "per-user") [ "build-fact" "per-user" "device" ]) "settings ${id}: stratum must be build-fact | per-user | device";
    assert lib.assertMsg (lib.elem mutability [ "mutable" "immutable" ]) "settings ${id}: mutability must be mutable | immutable";
    {
      inherit id;
      inherit (s) schema key;
      option = lib.concatStringsSep "." path;
      description = lib.trim (opt.description or "");
      default = getAttrFromPath path config;
      class = s.class or "preference";
      stratum = s.stratum or "per-user";
      inherit mutability;
      locked = lib.elem id locks || (s.stratum or "per-user") == "build-fact";
      apply = s.apply or "live";
    }
    // typeRecord opt.type
    // optionalAttrs (s ? range) { inherit (s) range; };

  # A template's key record (no option behind it; the default is declared in the template).
  templateKeyRecord = tname: kname: k:
    {
      key = kname;
      description = k.description;
      inherit (k) default class mutability apply;
      locked = false;
    }
    // { type = k.type; }
    // optionalAttrs (k.type == "enum") { inherit (k) values; }
    // optionalAttrs (k.range != null) { inherit (k) range; };

  # The artifact.
  generate = { options, config, templates, schemaVersions, locks }:
    let
      keys = map (keyRecord { inherit locks config; }) (collect [ "mura" ] options.mura);
      schemas = lib.unique (map (k: k.schema) keys);
      unknownLocks = lib.subtractLists (map (k: k.id) keys) locks;
    in
    assert lib.assertMsg (unknownLocks == [ ]) "mura.settings.locks names keys that are not exported: ${toString unknownLocks}";
    {
      artifactVersion = 1;
      inherit keys;
      schemaVersions = lib.genAttrs schemas (s: schemaVersions.${s} or 1);
      templates = lib.mapAttrs
        (tname: t: {
          schemaVersion = t.schemaVersion;
          keys = mapAttrsToList (templateKeyRecord tname) t.keys;
        })
        templates;
    };
in
{
  inherit mkSetting collect generate;
}
