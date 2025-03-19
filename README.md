# OmniPaxos Demo
This is a small demo of how to transform a simple single-server RocksDB service into a distributed version using OmniPaxos.

Related resources:
- [Blog post](https://omnipaxos.com/blog/building-distributed-rocksdb-with-omnipaxos-in-8-minutes/)
- [Video](https://youtu.be/4VqB0-KOsms)

## Requirements
We used the following:
- Docker v.4.19.0 (for demo)
- Rust 1.71.0 (for development)



# Demos
Build images and start containers in detached mode:
```bash
$ docker compose up --build -d
```
(Note: Running this command for the first time can take a couple of minutes)

Attach to the client (`network-actor`) to send requests to the cluster:
```bash
$ docker attach network-actor
```

# Some Commands

```bash
kubectl delete all --all
kubectl delete configmap kv-config
```

It is also possible to run with kubernetes:
```bash
$ kubectl create -f kube.yml 
```
Attach to the client (`network-actor`) to send requests to the cluster:
```bash
$ kubectl attach -it net
```
# Reconfiguration

# 1. Patch the ConfigMap to include the new node (for example, add node 4)
kubectl patch configmap kv-config --type merge -p '{"data":{"NODES":"[1,2,3,4]"}}'
$ kubectl patch configmap kv-config -n default --type merge --patch-file=patch.json

# 2. Patch the ConfigMap to update the configuration ID (increment from, say, 1 to 2)

kubectl patch configmap kv-config --type merge -p '{"data":{"CONFIG_ID":"2"}}'
$ kubectl patch configmap kv-config --type merge --patch-file=patchconfig.json


# 3. Scale the StatefulSet to 4 replicas (to start the new pod)
$ kubectl scale statefulset kv-store --replicas=4

# 4. Replicate command
$ reconfigure 4

# To check the config-id running 
$ kubectl get configmap kv-config -n default -o yaml
$ kubectl get configmap kv-config -n default -o jsonpath="{.data.CONFIG_ID}"



### Client
Example network-actor CLI command:
```
Type a command here <put/delete/get> <args>: put a 1
```
Asks the cluster to write { key: "a", value: "1" }.

To send a command to a specific server, include its port at the end of the command e.g.,
```
get a 8001
```
Reads the value associated with "a" from server `s1` listening on port 8001.

## Demo 0: Single server
(Make sure to `git checkout single-server` branch before running docker compose)
1. Propose some commands from client.
2. In another terminal, kill the server:
```bash
$ docker kill s1
```
3. In the client terminal, try sending commands again. No command succeeds because the only server is down.

## Demo 1: Fault-tolerance
(Make sure to `git checkout omnipaxos-replicated` branch before running docker compose)
1. Attach to a majority of the servers to observe the OmniPaxos log:
```bash
$ docker attach s2
```
```bash
$ docker attach s3
```
2. Repeat steps 1-3 from Demo 0 (kill `s1`). This time, in step 3, the commands should still be successful because we still have a majority of servers running (`s2` and `s3`).
3. Simulate disconnection between the remaining servers by pausing `s2`:
```bash
$ docker pause s2
```
4. Propose commands. ``Put`` and ``Delete`` will not be successful regardless of which server receives them because they cannot get committed in the log without being replicated by a majority. However, ``Get``s from `s3` still works since it's still running.
5. Propose multiple values to the same key at both servers, e.g.,
```
put a 2 8002
put a 3 8003
```
6. Unpause ``s2`` and see on the servers' terminals that the concurrent modifications are ordered identically in to the OmniPaxos log.
```bash
$ docker unpause s2
```

## Demo 2: Snapshot
(Make sure to `git checkout omnipaxos-snapshot` branch before running docker compose)
1. Attach to one of the servers e.g., ``s3`` to observe the OmniPaxos log:
```bash
$ docker attach s3
```
2. Propose 5 commands from the client and see how the entries get squashed into one snapshotted entry on the server. Propose 5 more commands to see the 5 new entries get snapshotted and merged with the old snapshot.

# Contribution and Contributor
1. Ritika Ritika -  Coding, Reporting, Testing 

 # Testing Carried out -
1. In case minikube is already running we need to do fresh deployement
```bash
$ minikube delete --all --purge  
```
2. Starting the code 
```bash
$ minikube start  
```
3. Deploying the code on Kubernetes - https://hub.docker.com/repository/docker/ritikaanand
```bash
$ docker build -t ritikaanand/kvsdemo:v10.1 .
$ docker push ritikaanand/kvsdemo:v10.1
```
4. Deploying yml file and checking nodes and pods
```bash
$ kubectl create -f kube.yml
$ kubectl get pods - 4 pods 1 serves as client the rest 3 as server
$ kubectl get nodes
```
5. PUT, GET, DELETE commands on/from the DB through pods
```bash
$ put a 1
$ get a 8001
$ delete a
```
6. RECONFIGUARTION logic testing

# a. Patch the ConfigMap to include the new node (for example, add node 4)
```bash
$ kubectl patch configmap kv-config -n default --type merge --patch-file=patch.json
```
# b. Patch the ConfigMap to update the configuration ID (increment from, say, 1 to 2)
```bash
$ kubectl patch configmap kv-config --type merge --patch-file=patchconfig.json
```

# c. Scale the StatefulSet to 4 replicas (to start the new pod)
```bash
$ kubectl scale statefulset kv-store --replicas=4
```
# d. Replicate command
```bash
$ reconfigure 4
```
7. Autoreconfiguration is also enabled using pod watcher.

# a. Patch the ConfigMap to include the new node (for example, add node 4)
```bash
$ kubectl patch configmap kv-config -n default --type merge --patch-file=patch.json
```
# b. Patch the ConfigMap to update the configuration ID (increment from, say, 1 to 2)
```bash
$ kubectl patch configmap kv-config --type merge --patch-file=patchconfig.json
```

# c. Scale the StatefulSet to 4 replicas (to start the new pod)
```bash
$ kubectl scale statefulset kv-store --replicas=4
```

