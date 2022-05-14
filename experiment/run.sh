int_handler() {
    echo "Interrupted."
    kill $PPID
    kill -9 ${meta_pid}
    kill -9 ${query_pid}
    aws --endpoint-url http://127.0.0.1:9900/ s3 rm --recursive s3://databend
    aws --endpoint-url http://127.0.0.1:9900/ s3 rb s3://databend
    exit 1
}
trap 'int_handler' INT

# start s3
export AWS_ACCESS_KEY_ID=minioadmin
export AWS_SECRET_ACCESS_KEY=minioadmin
export AWS_EC2_METADATA_DISABLED=true
aws --endpoint-url http://127.0.0.1:9900/ s3 mb s3://databend
echo "Created s3 bucket"

# start meta server
start_meta() {
  mkdir -p meta
  cd meta
  rm -r *
  RUST_LOG=trace ../../target/release/databend-meta -c ../databend-meta.toml 2>&1 > meta.log &
  ret_val=$!
  cd ..
}

# start query server
start_query() {
  mkdir -p query
  cd query
  rm -r *
  RUST_LOG=trace ../../target/release/databend-query -c ../databend-query.toml 2>&1 > query.log &
  ret_val=$!
  cd ..
}

start_meta
meta_pid=${ret_val}
echo "Started meta server at ${meta_pid}"
sleep 2

start_query
query_pid=${ret_val}
echo "Started query server at ${query_pid}"
sleep 5

mysql -uroot -h127.0.0.1 -P3307 < dumptpch-100m.sql &
insert_pid=$!
echo "Inserting TPC-H at ${insert_pid}"

# for ((i = 0; i < 6; i++)) do
#   sleep 10
#   kill -9 ${meta_pid}
#   sleep 10
#   start_meta
#   meta_pid=${ret_val}
#   echo "Restarted meta server at ${meta_pid}"

#   kill -9 ${insert_pid}
#   mysql -uroot -h127.0.0.1 -P3307 < dumptpch-100m.sql &
#   insert_pid=$!
#   echo "Re-inserting TPC-H at ${insert_pid}"
# done

for ((i = 0; i < 6; i++)) do
  sleep 10
  kill -9 ${query_pid}
  sleep 10
  start_query
  query_pid=${ret_val}
  echo "Restarted query server at ${query_pid}"

  kill -9 ${insert_pid}
  mysql -uroot -h127.0.0.1 -P3307 < dumptpch-100m.sql &
  insert_pid=$!
  echo "Re-inserting TPC-H at ${insert_pid}"
done

sleep 60
echo "Cleaning up"
kill -9 ${meta_pid}
kill -9 ${query_pid}
kill -9 ${insert_pid}

mkdir -p dep
cp meta/dependency_summary.jsons dep/meta_normal.jsons
cp query/dependency_summary.jsons dep/query_normal.jsons

aws --endpoint-url http://127.0.0.1:9900/ s3 rm --recursive s3://databend
aws --endpoint-url http://127.0.0.1:9900/ s3 rb s3://databend
