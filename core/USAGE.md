# NumiCoin One-Click Miner

## Overview

The NumiCoin One-Click Miner is a simplified, user-friendly version of the NumiCoin blockchain node that makes it incredibly easy for anyone to start mining cryptocurrency. Just download, double-click, and start earning!

## Features

✅ **Zero Configuration** - No setup required  
✅ **Auto Wallet Generation** - Creates your wallet automatically  
✅ **Instant Mining** - Starts mining immediately  
✅ **Real-time Progress** - Shows mining stats and earnings  
✅ **Self-contained** - Everything in one executable  
✅ **Cross-platform** - Works on Windows, macOS, and Linux  

## Quick Start

### For Users (Download & Mine)

1. **Download** the executable for your platform:
   - `NumiCoin-Miner.exe` (Windows)
   - `NumiCoin-Miner-macOS` (macOS)
   - `NumiCoin-Miner-Linux` (Linux)

2. **Run** the executable (double-click or run from terminal)

3. **Start Mining** - It will automatically:
   - Create your personal wallet
   - Initialize the blockchain
   - Begin mining with your CPU

4. **Monitor Progress** - Watch your earnings in real-time!

5. **Stop Mining** - Press `Ctrl+C` to stop and save

### What Gets Created

When you run the miner, it creates:
- `my-wallet.json` - Your personal wallet (KEEP THIS SAFE!)
- `numi-data/` - Blockchain data directory

## For Developers (Building)

### Build the One-Click Miner

```bash
# On Unix/Linux/macOS
cd core
./build-one-click.sh

# On Windows
cd core
build-one-click.bat
```

This creates a `dist/` folder with:
- The executable for your platform
- README.txt with user instructions

### Development Commands

```bash
# Build just the one-click binary
cargo build --release --bin numi-one-click

# Run the one-click miner directly
./target/release/numi-one-click

# Build for different targets
cargo build --release --target x86_64-pc-windows-gnu --bin numi-one-click
cargo build --release --target x86_64-apple-darwin --bin numi-one-click
cargo build --release --target x86_64-unknown-linux-gnu --bin numi-one-click
```

## Distribution

The one-click miner produces a **single executable file** that users can:
- Download from your website
- Share via email or cloud storage
- Run offline (solo mining)
- Use without any technical knowledge

## Example Output

```
🚀 NumiCoin One-Click Miner Starting...
========================================

🔑 Creating new wallet...
✅ Wallet saved to: ./my-wallet.json
💰 Your Wallet Address: 685dbc8a362ed477f9240df9313431af561f3bd59006edf1bb9525f0266c49bb
📁 Wallet File: ./my-wallet.json
📂 Data Directory: ./numi-data

🔧 Initializing blockchain...
📦 Loaded existing blockchain (height: 0)
💎 Current Balance: 0 NUMI

⛏️  Starting mining...
🔥 Using 10 CPU threads
⏱️  Target block time: 10 seconds

🎯 Mining started! Status updates every 15 seconds...
💡 Press Ctrl+C to stop mining and exit
============================================================

📊 Height: 1 | Difficulty: 8 | Balance: 0 NUMI | Blocks Mined: 0
🎉 NEW BLOCK MINED! Earned 50.0 NUMI
📊 Height: 2 | Difficulty: 8 | Balance: 50.0 NUMI | Blocks Mined: 1
```

## Security Notes

- **Keep your wallet safe** - The `my-wallet.json` file contains your private keys
- **Backup your wallet** - Copy it to a secure location
- **Don't share your wallet** - Anyone with the file can access your coins

## Technical Details

- **Algorithm**: Argon2 (memory-hard, ASIC-resistant)
- **Block Time**: 10 seconds (fast for testing)
- **Mining Reward**: 50 NUMI per block
- **CPU Mining**: Uses all available CPU cores
- **Storage**: ~100MB for blockchain data

## Troubleshooting

**Problem**: Executable won't run on macOS  
**Solution**: Run `chmod +x NumiCoin-Miner-macOS` to make it executable

**Problem**: Windows shows security warning  
**Solution**: Click "More info" → "Run anyway" (it's safe!)

**Problem**: Low mining performance  
**Solution**: Close other applications to free up CPU resources

**Problem**: "Address already in use" error  
**Solution**: Wait a few seconds and try again, or restart your computer

## Support

For questions or issues:
- Check the main README.md
- Visit the GitHub repository
- Create an issue for bugs or feature requests 