# start minio
docker run -d --rm -p 9900:9000 --name minio \
  -e "MINIO_ACCESS_KEY=minioadmin" \
  -e "MINIO_SECRET_KEY=minioadmin" \
  -v /tmp/data:/data \
  -v /tmp/config:/root/.minio \
  minio/minio server /data

unzip dumptpch-100m.sql.zip
