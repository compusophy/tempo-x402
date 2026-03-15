import json
import os

def main():
    try:
        with open('discovered_peers.json', 'r') as f:
            peers = json.load(f)
    except Exception:
        peers = []

    stats = {
        "total_peers": len(peers),
        "endpoints": {},
        "status": "active"
    }

    prices = []
    for peer in peers:
        for ep in peer.get('endpoints', []):
            price = ep.get('price', 0)
            name = ep.get('slug', 'unknown')
            if name not in stats['endpoints']:
                stats['endpoints'][name] = []
            stats['endpoints'][name].append(price)

    summary = {}
    for name, p_list in stats['endpoints'].items():
        summary[name] = {
            "avg_price": sum(p_list) / len(p_list) if p_list else 0,
            "count": len(p_list),
            "min_price": min(p_list) if p_list else 0,
            "max_price": max(p_list) if p_list else 0
        }
    
    stats['endpoints'] = summary
    print(json.dumps(stats))

if __name__ == "__main__":
    main()
