#!/usr/bin/env python3
import json
import hashlib

# Load the miner wallet
with open('test-wallets/miner-wallet.json', 'r') as f:
    wallet = json.load(f)

# Get the public key
public_key = bytes(wallet['public_key'])

# Derive address using Blake3 (simulate what the blockchain does)
# For now, let's use SHA256 as a simple hash
address = hashlib.sha256(public_key).digest()

print(f"Full public key length: {len(public_key)} bytes")
print(f"Full public key (first 64 chars): {public_key[:32].hex()}")
print(f"Derived address (32 bytes): {address.hex()}")
print(f"Derived address (64 bytes): {address.hex() + address.hex()}")

# Check if the user's address matches any part
user_address = "e002b3c9d7335cda3d8597c8e4bb20891d5571dc1ac30978413ad328d8a7a98162462bfd63df7fa54b5ca1d96c960676686deb2bf3db"
print(f"User provided address: {user_address}")
print(f"User address length: {len(user_address) // 2} bytes")

# Check if user address matches first 32 bytes of derived address
if user_address.startswith(address.hex()):
    print("✅ User address matches first 32 bytes of derived address")
else:
    print("❌ User address does not match derived address") 