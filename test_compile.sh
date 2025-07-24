#!/bin/bash
cd /home/boid/projects/syncread
cargo check 2>&1
echo "Exit code: $?"
