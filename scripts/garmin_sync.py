#!/usr/bin/env python3
"""Download only new Garmin Connect FIT activities with an atomic resume journal."""
import argparse, json, os, pathlib, tempfile
from datetime import datetime
from garminconnect import Garmin
import garth

def load(path):
    try: return json.loads(path.read_text())
    except FileNotFoundError: return {"version": 1, "completed_ids": [], "checkpoint": None}

def save(path, state):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("w", dir=path.parent, delete=False) as file:
        json.dump(state, file, sort_keys=True); file.flush(); os.fsync(file.fileno()); name=file.name
    os.replace(name, path)

def main():
    parser=argparse.ArgumentParser(); parser.add_argument("--output", type=pathlib.Path, required=True); parser.add_argument("--state", type=pathlib.Path, required=True); parser.add_argument("--email", required=True); parser.add_argument("--password", required=True); parser.add_argument("--mfa")
    args=parser.parse_args(); state=load(args.state); api=Garmin(args.email,args.password,is_cn=True)
    if args.mfa:
        api.garth.oauth1_token, api.garth.oauth2_token = garth.sso.login(args.email,args.password,client=api.garth,prompt_mfa=lambda: args.mfa)
        api.display_name=api.garth.profile["displayName"]
    else: api.login()
    completed=set(state["completed_ids"]); activities=api.get_activities(0,100)
    newest=state.get("checkpoint")
    for activity in sorted(activities, key=lambda value: value.get("startTimeGMT") or ""):
        start=activity.get("startTimeGMT"); identifier=str(activity["activityId"])
        if newest and start and start <= newest: continue
        if identifier in completed: continue
        state["pending_id"]=identifier; save(args.state,state)
        data=api.download_activity(activity["activityId"], dl_fmt=api.ActivityDownloadFormat.ORIGINAL)
        target=args.output / f"{identifier}.fit"; target.parent.mkdir(parents=True,exist_ok=True)
        with tempfile.NamedTemporaryFile("wb",dir=target.parent,delete=False) as file: file.write(data); file.flush(); os.fsync(file.fileno()); name=file.name
        os.replace(name,target); completed.add(identifier); state["completed_ids"]=sorted(completed); state["checkpoint"]=start or newest; state.pop("pending_id",None); save(args.state,state)
    print(json.dumps({"downloaded": len(completed), "checkpoint": state.get("checkpoint")}))
if __name__ == "__main__": main()
