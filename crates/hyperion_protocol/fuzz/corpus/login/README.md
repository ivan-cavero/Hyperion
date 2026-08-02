# Login corpus

The login target feeds arbitrary frames to the Login Start, Encryption
Response and Login Acknowledged decoders. These parsers handle clientbound
input from the network and must never panic on malformed usernames, UUIDs or
RSA-sized byte arrays.
