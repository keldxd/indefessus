# Security and responsible use

## Use Indefessus only on systems you own or are allowed to test

Indefessus sends test emails designed to trick AI agents. Run it only against a mailbox and agent that you own, or that you have written permission to test. The `verify` step enforces this for the target mailbox; please respect the same rule everywhere else.

## If a widely used product fails a test

If Indefessus shows that a product used by many organisations (an email assistant, an automation platform, an AI model provider) can be turned against its users:

1. **Report it privately to the vendor first**, through its security or bug bounty contact. Include the test file, the date, and the evidence from your report.
2. **Give the vendor reasonable time to fix it** (90 days is a common standard) before publishing details.
3. **Publish afterwards** so other organisations can protect themselves.

The maintainer is happy to help you write a report or reach a vendor.

## Reporting a vulnerability in Indefessus itself

Please do not open a public issue. Use GitHub's private vulnerability reporting on this repository (Security tab, "Report a vulnerability"). You will get a reply within 7 days.
