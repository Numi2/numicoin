#!/usr/bin/env python3
import json
import hashlib

def get_wallet_address(wallet_file):
    """Extract wallet address from wallet JSON file"""
    with open(wallet_file, 'r') as f:
        wallet_data = json.load(f)
    
    # Get public key as bytes
    public_key_bytes = bytes(wallet_data['public_key'])
    
    # Create address by hashing the public key
    address_hash = hashlib.sha256(public_key_bytes).hexdigest()
    
    print(f"Wallet file: {wallet_file}")
    print(f"Public key length: {len(public_key_bytes)} bytes")
    print(f"Address (SHA256 hash): {address_hash}")
    print(f"Address (first 32 chars): {address_hash[:32]}")
    
    return address_hash

if __name__ == "__main__":
    # Test with our wallets
    print("=== Miner Wallet ===")
    miner_address = get_wallet_address("./core-data/miner-wallet.json")
    
    print("\n=== Test Wallet ===")
    test_address = get_wallet_address("test-wallet.json")
    
    print(f"\n=== Summary ===")
    print(f"Miner address: {miner_address}")
    print(f"Test address: {test_address}") 