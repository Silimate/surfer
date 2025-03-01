#!/usr/bin/env bash
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
   args=()
   while [[ $# -gt 0 ]]; do
      case $1 in
         -c|--command-file|--script)
            SCRIPT=$(wslpath -w $2)
            shift # past argument
            shift # past value
            args+=("-c")
            args+=("$SCRIPT")
            ;;
         --spade-state)
            SPADE_STATE=$(wslpath -w $2)
            shift # past argument
            shift # past value
            args+=("--spade-state")
            args+=("$SPADE_STATE")
            ;;
         --spade-top)
            SPADE_TOP=$2
            shift # past argument
            shift # past value
            args+=("--spade-top")
            args+=("$SPADE_TOP")
            ;;
         -s|--state-file)
            STATE_FILE=$(wslpath -w $2)
            shift # past argument
            shift # past value
            args+=("-s")
            args+=("$STATE_FILE")
            ;;
         --help)
            shift # past argument
            args+=("--help")
            ;;
         -V|--version)
            shift # past argument
            args+=("-V")
            ;;
         -*|--*)
            echo "surfer.sh: Unknown option $1"
            exit 1
            ;;
         *)
            FILENAME=$(wslpath -w $1)
            shift
            args+=("$FILENAME")
            ;;
      esac
   done
   echo "Starting Surfer from WSL using:"
   echo $SURFERPATH ${args[@]}
   $SURFERPATH ${args[@]}
else
   echo "It looks like you are not in WSL. If you are in WSL, please open an issue at https://gitlab.com/surfer-project/surfer"
fi
