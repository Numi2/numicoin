#!/usr/bin/env python3
import json
import requests
import time

def test_transaction():
    """Test transaction submission via RPC API"""
    
    # RPC endpoint
    rpc_url = "http://localhost:8082/transaction"
    
    # Transaction data
    tx_data = {
        "from": "6d1f5146af55d9645991380d59ba8059da1f79e639b3f72fe1ec07c0820e0d1c",  # Miner address
        "to": "9fc748ade93ae6d6fc6c7b77359e9240d6314e1482846b45a00ef3a228a23290",   # Test wallet address
        "amount": 1,  # 1 smallest unit (0.01 NUMI)
        "nonce": 0,
        "fee": 1,  # 1 smallest unit fee
        "signature": "0000000000000000000000000000000000000000000000000000000000000000"  # Placeholder signature
    }
    
    print("Testing transaction submission...")
    print(f"From: {tx_data['from']}")
    print(f"To: {tx_data['to']}")
    print(f"Amount: {tx_data['amount']} smallest units ({tx_data['amount']/100:.2f} NUMI)")
    print(f"Fee: {tx_data['fee']} smallest units")
    
    try:
        response = requests.post(rpc_url, json=tx_data, timeout=10)
        print(f"Response status: {response.status_code}")
        print(f"Response: {response.text}")
        
        if response.status_code == 200:
            result = response.json()
            print(f"Transaction result: {json.dumps(result, indent=2)}")
        else:
            print(f"Error: {response.status_code} - {response.text}")
            
    except Exception as e:
        print(f"Error submitting transaction: {e}")

def check_balances():
    """Check balances of both wallets"""
    base_url = "http://localhost:8082/balance"
    
    miner_address = "6d1f5146af55d9645991380d59ba8059da1f79e639b3f72fe1ec07c0820e0d1c"
    test_address = "9fc748ade93ae6d6fc6c7b77359e9240d6314e1482846b45a00ef3a228a23290"
    
    print("\n=== Checking Balances ===")
    
    try:
        # Check miner balance
        response = requests.get(f"{base_url}/{miner_address}")
        if response.status_code == 200:
            miner_balance = response.json()
            print(f"Miner balance: {miner_balance['data']['balance']} NUMI")
        
        # Check test wallet balance
        response = requests.get(f"{base_url}/{test_address}")
        if response.status_code == 200:
            test_balance = response.json()
            print(f"Test wallet balance: {test_balance['data']['balance']} NUMI")
            
    except Exception as e:
        print(f"Error checking balances: {e}")

if __name__ == "__main__":
    print("=== NumiCoin Transaction Test ===")
    check_balances()
    test_transaction() 