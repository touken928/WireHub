FROM alpine:3.22
RUN apk add --no-cache python3 iproute2 wireguard-tools iputils
COPY client.py /client.py
ENTRYPOINT ["python3", "/client.py"]
CMD ["wait"]
