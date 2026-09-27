#!/bin/sh
set -eu

: "${REPORT_TO:?set REPORT_TO to the recipient address}"
cargo run --bin receipt_sender -- "$REPORT_TO" pay_demo_0042 12500 USD 120 settled

