#!/usr/bin/env python3
import json

# Read the test wallet
with open('test-wallet.json', 'r') as f:
    wallet = json.load(f)

# Extract public key and convert to hex
public_key = wallet['public_key']
hex_public_key = ''.join([f'{b:02x}' for b in public_key])

print(f"Test wallet public key (hex): {hex_public_key}")
print(f"Length: {len(hex_public_key)} characters") 