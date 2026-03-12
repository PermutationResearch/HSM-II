#!/usr/bin/env python3
"""
Vast.ai instance launcher for HSM-II training.
Automatically finds cheapest GPU and launches with proper configuration.
"""

import subprocess
import json
import sys
import time
from typing import Optional

def get_cheapest_gpu(min_vram_gb: int = 40, gpu_type: Optional[str] = None) -> dict:
    """Find cheapest available GPU on Vast.ai."""
    
    # Build search query
    query = f'gpu_ram >= {min_vram_gb * 1024}'
    if gpu_type:
        query += f' and gpu_name == "{gpu_type}"'
    
    cmd = ['vastai', 'search', 'offers', query, '--raw']
    
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, check=True)
        offers = json.loads(result.stdout)
        
        if not offers:
            print("No matching GPUs found")
            sys.exit(1)
        
        # Sort by price
        offers.sort(key=lambda x: float(x.get('dph_total', 999)))
        
        return offers[0]
        
    except subprocess.CalledProcessError as e:
        print(f"Error searching Vast.ai: {e}")
        sys.exit(1)
    except json.JSONDecodeError:
        print("Failed to parse Vast.ai response")
        sys.exit(1)

def launch_instance(offer: dict, image: str = "pytorch/pytorch:2.1.0-cuda12.1-cudnn8-runtime") -> str:
    """Launch Vast.ai instance with given offer."""
    
    # Read cloud init script
    with open('training/scripts/cloud_init_vastai.sh') as f:
        onstart_cmd = f.read()
    
    cmd = [
        'vastai', 'create', 'instance',
        str(offer['id']),
        '--image', image,
        '--disk', '100',
        '--onstart-cmd', onstart_cmd,
        '--env', '-e OLLAMA_HOST=0.0.0.0:11434',
        '--raw'
    ]
    
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, check=True)
        response = json.loads(result.stdout)
        
        if 'new_contract' in response:
            return response['new_contract']
        else:
            print(f"Unexpected response: {response}")
            sys.exit(1)
            
    except subprocess.CalledProcessError as e:
        print(f"Error launching instance: {e}")
        print(f"stderr: {e.stderr}")
        sys.exit(1)

def wait_for_instance(contract_id: str, timeout: int = 300) -> dict:
    """Wait for instance to become running."""
    
    print(f"Waiting for instance {contract_id} to be ready...")
    start = time.time()
    
    while time.time() - start < timeout:
        cmd = ['vastai', 'show', 'instances', '--raw']
        result = subprocess.run(cmd, capture_output=True, text=True)
        
        try:
            instances = json.loads(result.stdout)
            for inst in instances:
                if str(inst.get('id')) == contract_id:
                    if inst.get('actual_status') == 'running':
                        return inst
                    print(f"  Status: {inst.get('actual_status')}...")
                    break
        except json.JSONDecodeError:
            pass
        
        time.sleep(10)
    
    print("Timeout waiting for instance")
    sys.exit(1)

def main():
    import argparse
    
    parser = argparse.ArgumentParser(description='Launch HSM-II training on Vast.ai')
    parser.add_argument('--gpu', type=str, help='GPU type (e.g., A100, RTX 4090)')
    parser.add_argument('--vram', type=int, default=40, help='Minimum VRAM in GB')
    parser.add_argument('--max-price', type=float, help='Maximum $/hour')
    parser.add_argument('--auto-start', action='store_true', help='Auto-start training')
    
    args = parser.parse_args()
    
    print("🔍 Searching for cheapest GPU...")
    offer = get_cheapest_gpu(args.vram, args.gpu)
    
    price = float(offer.get('dph_total', 0))
    gpu_name = offer.get('gpu_name', 'Unknown')
    vram = int(offer.get('gpu_ram', 0)) / 1024
    
    print(f"\n💰 Found: {gpu_name} ({vram:.0f}GB) @ ${price:.2f}/hr")
    
    if args.max_price and price > args.max_price:
        print(f"❌ Price exceeds maximum (${args.max_price}/hr)")
        sys.exit(1)
    
    confirm = input("\nLaunch instance? [Y/n]: ").strip().lower()
    if confirm and confirm not in ['y', 'yes']:
        print("Cancelled")
        sys.exit(0)
    
    print("\n🚀 Launching instance...")
    contract_id = launch_instance(offer)
    
    print(f"Contract ID: {contract_id}")
    
    # Wait for instance
    instance = wait_for_instance(contract_id)
    
    print("\n✅ Instance ready!")
    print(f"  SSH: ssh root@{instance['public_ipaddr']} -p {instance['ports']['22/tcp'][0]['HostPort']}")
    print(f"  Cost: ${price:.2f}/hr")
    
    if not args.auto_start:
        print("\nTo start training:")
        print(f"  ssh root@{instance['public_ipaddr']} -p {instance['ports']['22/tcp'][0]['HostPort']}")
        print("  cd hyper-stigmergic-morphogenesisII")
        print("  ./training/scripts/start_training.sh")

if __name__ == '__main__':
    main()
