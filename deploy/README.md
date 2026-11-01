# Deploy (Frankfurt box)

```bash
sudo useradd -r -m -d /opt/solultimate solultimate
sudo -u solultimate git clone <repo> /opt/solultimate && cd /opt/solultimate
cargo build --release -p unode --features grpc
sudo mkdir -p /etc/solultimate && sudo chmod 700 /etc/solultimate
sudo cp deploy/*.service /etc/systemd/system/ && sudo systemctl daemon-reload
sudo systemctl enable --now recorder        # day 1: start recording
# only after the evaluation gate in docs/RUNBOOK.md passes:
sudo systemctl enable --now trader          # mode = "paper" first
```

Kill switch: `touch /opt/solultimate/STOP` sells everything and stops new entries; the
trader exits once flat.
