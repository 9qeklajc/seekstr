#!/bin/bash

# Script to resize profile images to 512x512 WebP format

PROFILE_DIR="profile"
OUTPUT_SIZE="512x512"

# Check if ImageMagick is installed
if ! command -v convert &> /dev/null; then
    echo "Error: ImageMagick is not installed. Please install it first."
    echo "On Debian/Ubuntu: sudo apt-get install imagemagick"
    echo "On macOS: brew install imagemagick"
    exit 1
fi

# Check if profile directory exists
if [ ! -d "$PROFILE_DIR" ]; then
    echo "Error: Profile directory '$PROFILE_DIR' not found"
    exit 1
fi

echo "Resizing profile images to $OUTPUT_SIZE WebP format..."

# Process each image in the profile directory
for img in "$PROFILE_DIR"/*.{png,jpg,jpeg,gif,bmp,svg,webp}; do
    # Skip if no files match
    [ -f "$img" ] || continue

    # Get filename without extension
    filename=$(basename "$img")
    name="${filename%.*}"

    # Output file path
    output="$PROFILE_DIR/${name}_512x512.webp"

    echo "Processing: $img"

    # Resize and convert to WebP
    # -resize 512x512^ : resize to fill 512x512 (may exceed in one dimension)
    # -gravity center : center the image
    # -extent 512x512 : crop to exactly 512x512
    # -quality 90 : WebP quality (0-100)
    convert "$img" \
        -resize 512x512^ \
        -gravity center \
        -extent 512x512 \
        -quality 90 \
        "$output"

    if [ $? -eq 0 ]; then
        echo "  ✓ Created: $output"
        echo "  Size: $(du -h "$output" | cut -f1)"
    else
        echo "  ✗ Failed to process $img"
    fi
done

echo ""
echo "Done! Resized images are in the $PROFILE_DIR directory with '_512x512.webp' suffix"