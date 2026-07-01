#!/bin/bash
PLIST="$HOME/Library/LaunchAgents/vault-agent.plist"
VAULT_BIN=$(which vault)

# Unload existing service if already running
if launchctl list | grep -q "vault-agent"; then
  launchctl bootout gui/$UID "$PLIST" 2>/dev/null
fi

cat > "$PLIST" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>vault-agent</string>
  <key>ProgramArguments</key><array>
    <string>$VAULT_BIN</string>
    <string>agent</string>
    <string>-config</string>
    <string>$HOME/.vault-agent-config.hcl</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
</dict></plist>
EOF

launchctl bootstrap gui/$UID "$PLIST"
echo "Vault Agent enabled and running."
