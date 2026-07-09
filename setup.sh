#!/bin/bash
set -e

echo "=> Setting up development environment for model2vec-api..."

echo "=> 1. Installing system dependencies (pkg-config, libssl-dev, build-essential)..."
if command -v apt-get >/dev/null; then
    sudo apt-get update
    sudo apt-get install -y pkg-config libssl-dev build-essential
else
    echo "Warning: apt-get not found. Please install pkg-config, libssl-dev, and a C compiler manually."
fi

echo "=> 2. Updating Rust to stable (Rust 2024 requires 1.85.0+)..."
if command -v rustup >/dev/null; then
    rustup update stable
    rustup default stable
else
    echo "Error: rustup not found. Please install Rust from https://rustup.rs/"
    exit 1
fi

echo "=> 3. Setting up pre-push git hook..."
if [ -d ".git" ]; then
    mkdir -p .git/hooks
    cp hooks/pre-push .git/hooks/pre-push
    chmod +x .git/hooks/pre-push
    echo "Pre-push hook installed."
else
    echo "Warning: .git directory not found. Skipping git hook installation."
fi

echo "=> 4. Setting up .env file..."
if [ ! -f .env ]; then
    if [ -f .env.example ]; then
        cp .env.example .env
        echo "Created .env from .env.example."
    else
        echo "Warning: .env.example not found."
    fi
else
    echo ".env already exists, skipping."
fi

echo "=> 5. Building the project to fetch dependencies..."
cargo build

echo "=> Setup complete! You are ready to develop."
