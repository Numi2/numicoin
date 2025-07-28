#!/usr/bin/env python3
import json
import hashlib

# Load the wallet file
with open('new-miner-wallet.json', 'r') as f:
    wallet_data = json.load(f)

# Extract public key
public_key_bytes = bytes(wallet_data['public_key'])

# Calculate wallet address (blake3 hash of public key)
wallet_address = hashlib.blake2b(public_key_bytes, digest_size=32).hexdigest()

print(f"Wallet Address: {wallet_address}")
print(f"Public Key (first 32 bytes): {public_key_bytes[:32].hex()}") 