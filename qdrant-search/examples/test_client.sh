#!/bin/bash

BASE_URL="http://localhost:3009"

echo "🔍 Testing Qdrant Search Server"
echo "================================"

# Test health endpoint
echo "1. Testing health endpoint..."
curl -s "$BASE_URL/health" | jq '.'
echo ""

# Post a sample event
echo "2. Posting sample event..."
curl -X POST "$BASE_URL/events" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "abcd1234567890ef1234567890abcdef1234567890abcdef1234567890abcdef",
    "pubkey": "npub1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcd",
    "created_at": 1759479359,
    "kind": 1,
    "tags": [["t", "rust"], ["t", "programming"]],
    "content": "This is a sample Nostr event about Rust programming and vector databases.",
    "sig": "signature_placeholder_1234567890abcdef1234567890abcdef1234567890"
  }'
echo ""

# Post another sample event
echo "3. Posting another sample event..."
curl -X POST "$BASE_URL/events" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "ef1234567890abcd1234567890abcdef1234567890abcdef1234567890abcdef",
    "pubkey": "npub9876543210fedcba9876543210fedcba9876543210fedcba9876543210fe",
    "created_at": 1759475759,
    "kind": 1,
    "tags": [["t", "database"], ["t", "ai"]],
    "content": "Vector databases are fascinating for semantic search and AI applications.",
    "sig": "signature_placeholder_9876543210fedcba9876543210fedcba9876543210"
  }'
echo ""

# Wait a moment for processing
echo "4. Waiting for events to be processed..."
sleep 3

# Test semantic search
echo "5. Testing semantic search for 'vector databases'..."
curl -s "$BASE_URL/search?query=vector%20databases&limit=10" | jq '.'
echo ""

echo "6. Testing semantic search for 'Rust programming'..."
curl -s "$BASE_URL/search?query=Rust%20programming&limit=10" | jq '.'
echo ""

# Test event search with filters
echo "7. Testing event search with kind filter..."
curl -s "$BASE_URL/events?event_kinds=%5B1%5D&limit=10" | jq '.'
echo ""

echo "✅ Testing complete!"