{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.services.cdk-mintd;

  settingsFormat = pkgs.formats.toml { };

  configFile = settingsFormat.generate "cdk-mintd.toml" cfg.settings;

  # Environment file for secrets
  envFile = pkgs.writeText "cdk-mintd.env" (concatStringsSep "\n"
    (mapAttrsToList (name: value: "${name}=${toString value}")
      cfg.environment));

in
{
  options.services.cdk-mintd = {
    enable = mkEnableOption "CDK Mint Daemon";

    package = mkOption {
      type = types.package;
      default = pkgs.cdk-mintd;
      description = "The cdk-mintd package to use";
    };

    user = mkOption {
      type = types.str;
      default = "cdk-mintd";
      description = "User account under which cdk-mintd runs";
    };

    group = mkOption {
      type = types.str;
      default = "cdk-mintd";
      description = "Group under which cdk-mintd runs";
    };

    dataDir = mkOption {
      type = types.path;
      default = "/var/lib/cdk-mintd";
      description = "Directory where cdk-mintd stores its data";
    };

    configFile = mkOption {
      type = types.nullOr types.path;
      default = null;
      description =
        "Path to the configuration file. If null, will be generated from settings";
    };

    settings = mkOption {
      type = types.attrs;
      default = { };
      example = literalExpression ''
        {
          info = {
            url = "http://127.0.0.1:3338";
            listen_host = "127.0.0.1";
            listen_port = 3338;
          };
          mint_info = {
            name = "My CDK Mint";
            description = "A Cashu mint powered by CDK";
          };
          ln = {
            ln_backend = "FakeWallet";
            mint_max = 10000;
            melt_max = 10000;
          };
          database = {
            engine = "sqlite";
          };
        }
      '';
      description = "Configuration for cdk-mintd as a Nix attribute set";
    };

    environment = mkOption {
      type = types.attrsOf types.str;
      default = { };
      example = {
        CDK_MINTD_LISTEN_HOST = "127.0.0.1";
        CDK_MINTD_LISTEN_PORT = "3338";
      };
      description = "Environment variables for cdk-mintd";
    };

    openFirewall = mkOption {
      type = types.bool;
      default = false;
      description = "Whether to open the firewall for the mint port";
    };

    logLevel = mkOption {
      type = types.enum [ "error" "warn" "info" "debug" "trace" ];
      default = "info";
      description = "Log level for cdk-mintd";
    };

    extraArgs = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "Extra command-line arguments to pass to cdk-mintd";
    };

    mnemonicFile = mkOption {
      type = types.nullOr types.path;
      default = null;
      description =
        "Path to file containing the mnemonic seed phrase. If specified, the mnemonic will be loaded from this file at startup.";
    };
  };

  config = mkIf cfg.enable {
    # User and group creation
    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.group;
      home = cfg.dataDir;
      createHome = true;
      description = "CDK Mint Daemon user";
    };

    users.groups.${cfg.group} = { };

    # Systemd service
    systemd.services.cdk-mintd = {
      description = "CDK Mint Daemon";
      after = [ "network.target" ];
      wantedBy = [ "multi-user.target" ];

      serviceConfig = {
        Type = "simple";
        User = cfg.user;
        Group = cfg.group;
        WorkingDirectory = cfg.dataDir;

        # Security settings
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ReadWritePaths = [ cfg.dataDir ];
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictSUIDSGID = true;
        RestrictRealtime = true;
        RestrictNamespaces = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;

        # Restart settings
        Restart = "always";
        RestartSec = "10s";

        # Environment
        Environment = [ "RUST_LOG=${cfg.logLevel}" ];
        EnvironmentFile = mkIf (cfg.environment != { }) envFile;
      };

      # Script to load mnemonic from file (if specified) and start cdk-mintd
      script = ''
        ${optionalString (cfg.mnemonicFile != null) ''
          if [ -f "${cfg.mnemonicFile}" ]; then
            export CDK_MINTD_MNEMONIC="$(cat "${cfg.mnemonicFile}")"
            echo "Loaded mnemonic from ${cfg.mnemonicFile}"
          else
            echo "Mnemonic file ${cfg.mnemonicFile} not found"
            exit 1
          fi
        ''}

        # Start cdk-mintd
        exec ${
          concatStringsSep " " ([
            "${cfg.package}/bin/cdk-mintd"
            "--work-dir ${cfg.dataDir}"
            "--config ${
              if cfg.configFile != null then cfg.configFile else configFile
            }"
          ] ++ cfg.extraArgs)
        }
      '';
    };

    # Create data directory with proper permissions using systemd.tmpfiles
    systemd.tmpfiles.rules =
      [ "d ${cfg.dataDir} 0750 ${cfg.user} ${cfg.group} -" ];

    # Firewall configuration
    networking.firewall = mkIf cfg.openFirewall {
      allowedTCPPorts = [ (cfg.settings.info.listen_port or 3338) ];
    };

    # Default configuration if none provided
    services.cdk-mintd.settings = mkDefault {
      info = {
        url = "http://127.0.0.1:3338";
        listen_host = "127.0.0.1";
        listen_port = 3338;
      };
      mint_info = {
        name = "CDK Mint";
        description = "A Cashu mint powered by CDK";
      };
      ln = {
        ln_backend = "FakeWallet";
        mint_max = 10000;
        melt_max = 10000;
      };
      database = { engine = "sqlite"; };
    };

    # Assertions for configuration validation
    assertions = [
      {
        assertion = cfg.settings ? info && cfg.settings.info ? listen_port;
        message = "services.cdk-mintd.settings.info.listen_port must be specified";
      }
      {
        assertion = cfg.settings ? ln && cfg.settings.ln ? ln_backend;
        message = "services.cdk-mintd.settings.ln.ln_backend must be specified";
      }
      {
        assertion = cfg.settings ? database && cfg.settings.database ? engine;
        message =
          "services.cdk-mintd.settings.database.engine must be specified";
      }
    ];
  };
}
