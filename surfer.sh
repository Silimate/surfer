#!/usr/bin/env sh
#
# Script to start Surfer from WSL
#
# 1. Edit SURFERPATH below to point to the Windows Surfer binary.
#    This is probably located in /mnt/c/...
# 2. Create a link from a directory in your PATH to this file, e.g.:
#         ln -s /usr/local/bin/surfer ./surfer.sh
# 3. Run "surfer filename" and the Windows Surfer should open with a file from WSL.
#
# Report any issues to https://gitlab.com/surfer-project/surfer

if [ -n "$WSL_DISTRO_NAME" ]; then
   # Add path to Windows surfer.exe
   SURFERPATH=/mnt/c/...
   FILENAME=$(wslpath -w $1)
   $SURFERPATH $FILENAME
else
   echo "It looks like you are not in WSL. If you are in WSL, please open an issue at https://gitlab.com/surfer-project/surfer"
fi
